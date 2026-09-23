#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    sync::{Arc, Mutex as StdMutex, MutexGuard, OnceLock, Weak},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BoundCanonicalDispatchError,
    BridgeActionKind, BridgeActionResult, CanonicalQueuedAction, LiveBridge,
};
use localview_postcondition_contracts::{
    PostconditionContractRegistry, RegisteredPostconditionContract,
    WebSemanticPostconditionEvaluation,
};
use localview_protocol::{PageSnapshot, PrincipalRef, SemanticNode, SessionId};
use localview_sessions::SessionManager;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    ControlState,
    fresh_snapshot::acquire_fresh_semantic_snapshot,
    resource_runtime::{
        ManagedSurfaceActionAuthority,
        current_primary_managed_surface_action_authority_for_sessions,
    },
};

const MAX_PENDING_MANAGED_CONSEQUENTIAL_PLANS: usize = 64;
const MAX_POSTCONDITION_CONTRACTS: usize = 8;
const MAX_POSTCONDITION_CONTRACT_REF_BYTES: usize = 4 * 1024;
const MAX_REFERENCE_BYTES: usize = 256;
const CONFIRMATION_TTL: Duration = Duration::from_secs(30);
const RECONCILIATION_RECORD_TTL: Duration = Duration::from_secs(5 * 60);
const DECISION_PRINCIPAL_REF: &str =
    "principal:local-control:bearer-holder-explicit-confirmation-v1";
const ACTING_PRINCIPAL_REF: &str = "principal:localview-daemon:managed-webview-v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedConsequentialPlanRequest {
    reference: String,
    action: BridgeActionKind,
    expected_postcondition_contract_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedConsequentialConfirmRequest {
    confirmation_ref: Uuid,
}

#[derive(Debug, Clone)]
struct PendingManagedConsequentialPlan {
    confirmation_ref: Uuid,
    queued: CanonicalQueuedAction,
    surface_authority: ManagedSurfaceActionAuthority,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct ManagedConsequentialReconciliation {
    session_id: SessionId,
    surface_authority: ManagedSurfaceActionAuthority,
    expected_postcondition_contract_refs: Vec<String>,
    status: &'static str,
    proof_ref: Option<String>,
    snapshot_version: Option<u64>,
    snapshot_route: Option<String>,
    detail: Option<String>,
    expires_at: Instant,
}

#[derive(Clone)]
struct ManagedConsequentialControlHandle {
    pending: Arc<Mutex<HashMap<Uuid, PendingManagedConsequentialPlan>>>,
    reconciliations: Arc<Mutex<HashMap<Uuid, ManagedConsequentialReconciliation>>>,
    plan_gate: Arc<Mutex<()>>,
}

struct ManagedConsequentialControlEntry {
    owner: Weak<SessionManager>,
    handle: ManagedConsequentialControlHandle,
}

type ManagedConsequentialControlRegistry = HashMap<usize, ManagedConsequentialControlEntry>;

static MANAGED_CONSEQUENTIAL_CONTROL: OnceLock<StdMutex<ManagedConsequentialControlRegistry>> =
    OnceLock::new();

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/managed-consequential/plan",
            post(plan_managed_consequential_action),
        )
        .route(
            "/v1/sessions/{id}/managed-consequential/{action_id}/confirm",
            post(confirm_managed_consequential_action),
        )
        .route(
            "/v1/sessions/{id}/managed-consequential/{action_id}/status",
            get(managed_consequential_status),
        )
        .with_state(state)
}

pub async fn release_managed_consequential_control_session_for_sessions(
    sessions: &Arc<SessionManager>,
    session_id: SessionId,
) {
    let Some(handle) = existing_control_for_sessions(sessions) else {
        return;
    };
    let _gate = handle.plan_gate.lock().await;
    handle
        .pending
        .lock()
        .await
        .retain(|_, plan| plan.queued.action.session_id != session_id);
    handle
        .reconciliations
        .lock()
        .await
        .retain(|_, record| record.session_id != session_id);
}

async fn plan_managed_consequential_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(session_id): Path<SessionId>,
    Json(request): Json<ManagedConsequentialPlanRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(session_id).await.is_none() {
        return error(StatusCode::NOT_FOUND, "session_not_found");
    }
    if !valid_reference(&request.reference) {
        return error(
            StatusCode::BAD_REQUEST,
            "managed_consequential_invalid_reference",
        );
    }
    if !allowed_action(&request.action) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "managed_consequential_action_not_supported",
                "supported": ["click", "focus"],
                "dispatch_performed": false,
                "confirmation_created": false,
            })),
        )
            .into_response();
    }
    if !valid_postcondition_refs(&request.expected_postcondition_contract_refs) {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "managed_consequential_invalid_postcondition_contract",
        );
    }

    let control = control_for_sessions(&state.sessions);
    let _plan_gate = control.plan_gate.lock().await;
    prune_expired(&control, &state.live).await;
    if control.pending.lock().await.len() >= MAX_PENDING_MANAGED_CONSEQUENTIAL_PLANS {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "managed_consequential_plan_capacity_exhausted",
                "max_pending_plans": MAX_PENDING_MANAGED_CONSEQUENTIAL_PLANS,
            })),
        )
            .into_response();
    }

    let authority_before = match current_primary_managed_surface_action_authority_for_sessions(
        &state.sessions,
        session_id,
    ) {
        Ok(authority) => authority,
        Err(code) => return error(StatusCode::CONFLICT, code),
    };

    // Fresh semantic evidence is acquired through the exact managed executor.
    // The worker's take path establishes the daemon-derived managed observation
    // lineage. A plan cannot be created from cached observer history.
    let snapshot = match acquire_fresh_semantic_snapshot(&state, session_id).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "managed_consequential_fresh_snapshot_required",
                    "dispatch_performed": false,
                    "confirmation_created": false,
                })),
            )
                .into_response();
        }
    };

    let authority_after = match current_primary_managed_surface_action_authority_for_sessions(
        &state.sessions,
        session_id,
    ) {
        Ok(authority) => authority,
        Err(code) => return error(StatusCode::CONFLICT, code),
    };
    if authority_before != authority_after {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_surface_authority_changed",
        );
    }

    // Generic DOM authority is accepted only when observation lineage is the
    // daemon-derived managed-WebView lineage for this exact surface. An
    // independent native provider is preserved, but it cannot be laundered into
    // DOM authority.
    let Some(observation) = state.live.observation_status(session_id).await else {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_observation_authority_missing",
        );
    };
    if observation.provider_incarnation_ref != authority_after.provider_incarnation_ref
        || observation.target_incarnation_ref != authority_after.target_incarnation_ref
    {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_independent_provider_observation",
        );
    }

    let Some(node) = find_node(&snapshot.root, &request.reference) else {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_reference_not_in_fresh_snapshot",
        );
    };
    if !node.interactive {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_reference_not_interactive",
        );
    }

    let Some(precondition_snapshot_cut_ref) =
        snapshot_cut_ref(&snapshot, &request.reference, &authority_after)
    else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "managed_consequential_snapshot_cut_failed",
        );
    };

    let confirmation_ref = Uuid::new_v4();
    let authorization_revision_ref = Uuid::new_v4();
    let queued = match state
        .live
        .bind_direct_canonical_action(
            session_id,
            Some(request.reference.clone()),
            request.action.clone(),
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from(DECISION_PRINCIPAL_REF),
                acting_principal_ref: PrincipalRef::from(ACTING_PRINCIPAL_REF),
                authorization_revision: format!(
                    "authorization:managed-webview:confirmation-v1:{authorization_revision_ref}"
                ),
                precondition_snapshot_cut_ref: precondition_snapshot_cut_ref.clone(),
                provider_incarnation_ref: authority_after.provider_incarnation_ref.clone(),
                target_incarnation_ref: authority_after.target_incarnation_ref.clone(),
                risk_class: ActionRiskClass::Unknown,
                idempotency_class: ActionIdempotencyClass::Unknown,
                expected_postcondition_contract_refs: request
                    .expected_postcondition_contract_refs
                    .clone(),
            },
        )
        .await
    {
        Ok(queued) => queued,
        Err(_) => {
            return error(
                StatusCode::CONFLICT,
                "managed_consequential_canonical_binding_rejected",
            );
        }
    };

    let action_id = queued.action.id;
    control.pending.lock().await.insert(
        action_id,
        PendingManagedConsequentialPlan {
            confirmation_ref,
            queued,
            surface_authority: authority_after.clone(),
            expires_at: Instant::now() + CONFIRMATION_TTL,
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "action_id": action_id,
            "confirmation_ref": confirmation_ref,
            "action": request.action,
            "reference": request.reference,
            "precondition_snapshot_cut_ref": precondition_snapshot_cut_ref,
            "provider_incarnation_ref": authority_after.provider_incarnation_ref,
            "target_incarnation_ref": authority_after.target_incarnation_ref,
            "expected_postcondition_contract_refs": request.expected_postcondition_contract_refs,
            "confirmation_expires_ms": CONFIRMATION_TTL.as_millis(),
            "dispatch_performed": false,
            "restart_restores_confirmation_authority": false,
        })),
    )
        .into_response()
}

async fn confirm_managed_consequential_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((session_id, action_id)): Path<(SessionId, Uuid)>,
    Json(request): Json<ManagedConsequentialConfirmRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(session_id).await.is_none() {
        return error(StatusCode::NOT_FOUND, "session_not_found");
    }

    let control = control_for_sessions(&state.sessions);
    let _plan_gate = control.plan_gate.lock().await;

    let plan = {
        let mut pending = control.pending.lock().await;
        let Some(existing) = pending.get(&action_id) else {
            return error(
                StatusCode::CONFLICT,
                "managed_consequential_confirmation_missing",
            );
        };
        if existing.queued.action.session_id != session_id {
            return error(
                StatusCode::CONFLICT,
                "managed_consequential_confirmation_session_mismatch",
            );
        }
        if existing.expires_at <= Instant::now() {
            let expired = pending
                .remove(&action_id)
                .expect("pending plan existed immediately before removal");
            drop(pending);
            state
                .live
                .discard_bound_canonical_action(expired.queued.action.id)
                .await;
            return error(
                StatusCode::GONE,
                "managed_consequential_confirmation_expired",
            );
        }
        if existing.confirmation_ref != request.confirmation_ref {
            return error(
                StatusCode::CONFLICT,
                "managed_consequential_confirmation_mismatch",
            );
        }
        pending
            .remove(&action_id)
            .expect("pending plan existed immediately before one-shot consume")
    };

    let current_authority = match current_primary_managed_surface_action_authority_for_sessions(
        &state.sessions,
        session_id,
    ) {
        Ok(authority) => authority,
        Err(code) => {
            state
                .live
                .discard_bound_canonical_action(plan.queued.action.id)
                .await;
            return error(StatusCode::CONFLICT, code);
        }
    };
    if current_authority != plan.surface_authority {
        state
            .live
            .discard_bound_canonical_action(plan.queued.action.id)
            .await;
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_surface_authority_changed",
        );
    }

    let observation_matches =
        state
            .live
            .observation_status(session_id)
            .await
            .is_some_and(|status| {
                status.provider_incarnation_ref == current_authority.provider_incarnation_ref
                    && status.target_incarnation_ref == current_authority.target_incarnation_ref
            });
    if !observation_matches {
        state
            .live
            .discard_bound_canonical_action(plan.queued.action.id)
            .await;
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_observation_authority_changed",
        );
    }

    let reconciliation = ManagedConsequentialReconciliation {
        session_id,
        surface_authority: plan.surface_authority.clone(),
        expected_postcondition_contract_refs: plan
            .queued
            .envelope
            .metadata
            .expected_postcondition_contract_refs
            .clone(),
        status: "pending_executor_completion",
        proof_ref: None,
        snapshot_version: None,
        snapshot_route: None,
        detail: None,
        expires_at: Instant::now() + RECONCILIATION_RECORD_TTL,
    };
    control
        .reconciliations
        .lock()
        .await
        .insert(action_id, reconciliation);

    match state
        .live
        .enqueue_bound_canonical_action_for_dispatch(plan.queued)
        .await
    {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "action_id": action_id,
                "confirmation_consumed": true,
                "queued_for_exact_managed_surface": true,
                "postcondition_status": "pending_executor_completion",
                "status_endpoint": format!(
                    "/v1/sessions/{session_id}/managed-consequential/{action_id}/status"
                ),
                "restart_restores_confirmation_authority": false,
            })),
        )
            .into_response(),
        Err(error_code) => {
            control.reconciliations.lock().await.remove(&action_id);
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": dispatch_error_code(error_code),
                    "action_id": action_id,
                    "confirmation_consumed": true,
                    "dispatch_performed": false,
                    "retry_same_confirmation_allowed": false,
                })),
            )
                .into_response()
        }
    }
}


async fn managed_consequential_status(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((session_id, action_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let Some(control) = existing_control_for_sessions(&state.sessions) else {
        return error(StatusCode::NOT_FOUND, "managed_consequential_status_not_found");
    };
    let records = control.reconciliations.lock().await;
    let Some(record) = records.get(&action_id) else {
        return error(StatusCode::NOT_FOUND, "managed_consequential_status_not_found");
    };
    if record.session_id != session_id {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_status_session_mismatch",
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "action_id": action_id,
            "postcondition_status": record.status,
            "expected_postcondition_contract_refs": record.expected_postcondition_contract_refs,
            "proof_ref": record.proof_ref,
            "fresh_snapshot_version": record.snapshot_version,
            "fresh_snapshot_route": record.snapshot_route,
            "detail": record.detail,
            "terminal": !matches!(
                record.status,
                "pending_executor_completion" | "pending_fresh_reconciliation"
            ),
        })),
    )
        .into_response()
}

pub(crate) fn schedule_managed_consequential_reconciliation(
    state: ControlState,
    session_id: SessionId,
    result: BridgeActionResult,
) {
    let Some(control) = existing_control_for_sessions(&state.sessions) else {
        return;
    };
    let action_id = result.action_id;
    tokio::spawn(async move {
        let initial = {
            let mut records = control.reconciliations.lock().await;
            let Some(record) = records.get_mut(&action_id) else {
                return;
            };
            if record.session_id != session_id {
                return;
            }
            record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
            if !result.ok {
                record.status = "executor_failed";
                record.detail = result.error.map(|value| bounded_detail(&value));
                return;
            }
            record.status = "pending_fresh_reconciliation";
            record.clone()
        };

        if current_primary_managed_surface_action_authority_for_sessions(
            &state.sessions,
            session_id,
        )
        .ok()
        .as_ref()
            != Some(&initial.surface_authority)
        {
            set_reconciliation_required(
                &control,
                action_id,
                "managed_surface_authority_changed_before_reconciliation",
            )
            .await;
            return;
        }

        let observation_matches = state
            .live
            .observation_status(session_id)
            .await
            .is_some_and(|status| {
                status.provider_incarnation_ref
                    == initial.surface_authority.provider_incarnation_ref
                    && status.target_incarnation_ref
                        == initial.surface_authority.target_incarnation_ref
            });
        if !observation_matches {
            set_reconciliation_required(
                &control,
                action_id,
                "managed_observation_lineage_changed_before_reconciliation",
            )
            .await;
            return;
        }

        let snapshot = match acquire_fresh_semantic_snapshot(&state, session_id).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                set_reconciliation_required(
                    &control,
                    action_id,
                    "fresh_post_dispatch_snapshot_unavailable",
                )
                .await;
                return;
            }
        };

        if current_primary_managed_surface_action_authority_for_sessions(
            &state.sessions,
            session_id,
        )
        .ok()
        .as_ref()
            != Some(&initial.surface_authority)
        {
            set_reconciliation_required(
                &control,
                action_id,
                "managed_surface_authority_changed_during_reconciliation",
            )
            .await;
            return;
        }

        let registry = PostconditionContractRegistry::standard();
        let mut verdicts = Vec::with_capacity(initial.expected_postcondition_contract_refs.len());
        let mut any_fail = false;
        let mut any_unknown = false;
        for contract_ref in &initial.expected_postcondition_contract_refs {
            let verdict = match registry.evaluate_web_semantic(contract_ref, &snapshot) {
                Ok(WebSemanticPostconditionEvaluation::VerifiedPass) => "verified_pass",
                Ok(WebSemanticPostconditionEvaluation::VerifiedFail) => {
                    any_fail = true;
                    "verified_fail"
                }
                Ok(WebSemanticPostconditionEvaluation::Unknown) | Err(_) => {
                    any_unknown = true;
                    "unknown"
                }
            };
            verdicts.push((contract_ref.as_str(), verdict));
        }

        let status = if any_fail {
            "verified_unexpected"
        } else if any_unknown {
            "reconciliation_required"
        } else {
            "verified_expected"
        };
        let proof_ref = reconciliation_proof_ref(
            action_id,
            &snapshot,
            &initial.surface_authority,
            &verdicts,
        );

        let mut records = control.reconciliations.lock().await;
        if let Some(record) = records.get_mut(&action_id) {
            record.status = status;
            record.proof_ref = proof_ref;
            record.snapshot_version = Some(snapshot.version);
            record.snapshot_route = Some(snapshot.route.clone());
            record.detail = (status == "reconciliation_required")
                .then(|| "one_or_more_postconditions_unresolved".to_owned());
            record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
        }
    });
}

async fn set_reconciliation_required(
    control: &ManagedConsequentialControlHandle,
    action_id: Uuid,
    detail: &'static str,
) {
    let mut records = control.reconciliations.lock().await;
    if let Some(record) = records.get_mut(&action_id) {
        record.status = "reconciliation_required";
        record.detail = Some(detail.to_owned());
        record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
    }
}

fn reconciliation_proof_ref(
    action_id: Uuid,
    snapshot: &PageSnapshot,
    authority: &ManagedSurfaceActionAuthority,
    verdicts: &[(&str, &str)],
) -> Option<String> {
    let encoded = serde_json::to_vec(snapshot).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"localview-managed-webview-postcondition-v1\0");
    digest.update(action_id.as_bytes());
    digest.update(b"\0");
    digest.update(encoded);
    digest.update(b"\0");
    digest.update(authority.provider_incarnation_ref.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(authority.target_incarnation_ref.as_str().as_bytes());
    for (contract_ref, verdict) in verdicts {
        digest.update(b"\0");
        digest.update(contract_ref.as_bytes());
        digest.update(b"=");
        digest.update(verdict.as_bytes());
    }
    let digest = digest.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").ok()?;
    }
    Some(format!("proof:managed-webview:sha256:{hex}"))
}

fn bounded_detail(value: &str) -> String {
    value.chars().take(512).collect()
}

fn control_for_sessions(sessions: &Arc<SessionManager>) -> ManagedConsequentialControlHandle {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = MANAGED_CONSEQUENTIAL_CONTROL.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries
        .entry(key)
        .or_insert_with(|| ManagedConsequentialControlEntry {
            owner: Arc::downgrade(sessions),
            handle: ManagedConsequentialControlHandle {
                pending: Arc::new(Mutex::new(HashMap::new())),
                reconciliations: Arc::new(Mutex::new(HashMap::new())),
                plan_gate: Arc::new(Mutex::new(())),
            },
        })
        .handle
        .clone()
}

fn existing_control_for_sessions(
    sessions: &Arc<SessionManager>,
) -> Option<ManagedConsequentialControlHandle> {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = MANAGED_CONSEQUENTIAL_CONTROL.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get(&key).map(|entry| entry.handle.clone())
}

async fn prune_expired(control: &ManagedConsequentialControlHandle, live: &LiveBridge) {
    let now = Instant::now();
    let expired = {
        let mut pending = control.pending.lock().await;
        let expired = pending
            .iter()
            .filter_map(|(action_id, plan)| (plan.expires_at <= now).then_some(*action_id))
            .collect::<Vec<_>>();
        for action_id in &expired {
            pending.remove(action_id);
        }
        expired
    };
    for action_id in expired {
        live.discard_bound_canonical_action(action_id).await;
    }
    control
        .reconciliations
        .lock()
        .await
        .retain(|_, record| record.expires_at > now);
}

fn allowed_action(action: &BridgeActionKind) -> bool {
    matches!(action, BridgeActionKind::Click | BridgeActionKind::Focus)
}

fn valid_reference(reference: &str) -> bool {
    let Some(hash) = reference.strip_prefix("@e") else {
        return false;
    };
    !hash.is_empty()
        && reference.len() <= MAX_REFERENCE_BYTES
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_postcondition_refs(refs: &[String]) -> bool {
    if refs.is_empty() || refs.len() > MAX_POSTCONDITION_CONTRACTS {
        return false;
    }
    let registry = PostconditionContractRegistry::standard();
    let mut unique = HashSet::with_capacity(refs.len());
    refs.iter().all(|contract_ref| {
        !contract_ref.is_empty()
            && contract_ref.len() <= MAX_POSTCONDITION_CONTRACT_REF_BYTES
            && !contract_ref.chars().any(char::is_control)
            && unique.insert(contract_ref.as_str())
            && matches!(
                registry.decode(contract_ref),
                Ok(RegisteredPostconditionContract::WebSemanticV1(_))
            )
    })
}

fn find_node<'a>(node: &'a SemanticNode, reference: &str) -> Option<&'a SemanticNode> {
    if node.reference == reference {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, reference))
}

fn snapshot_cut_ref(
    snapshot: &PageSnapshot,
    reference: &str,
    authority: &ManagedSurfaceActionAuthority,
) -> Option<String> {
    let encoded = serde_json::to_vec(snapshot).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"localview-managed-webview-precondition-v1\0");
    digest.update(encoded);
    digest.update(b"\0");
    digest.update(reference.as_bytes());
    digest.update(b"\0");
    digest.update(authority.provider_incarnation_ref.as_str().as_bytes());
    digest.update(b"\0");
    digest.update(authority.target_incarnation_ref.as_str().as_bytes());
    let digest = digest.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").ok()?;
    }
    Some(format!("cut:managed-webview:sha256:{hex}"))
}

fn dispatch_error_code(error: BoundCanonicalDispatchError) -> &'static str {
    match error {
        BoundCanonicalDispatchError::MissingCanonicalEnvelope => {
            "managed_consequential_binding_missing"
        }
        BoundCanonicalDispatchError::EnvelopeMismatch => "managed_consequential_binding_mismatch",
        BoundCanonicalDispatchError::ActionIdentityMismatch => {
            "managed_consequential_action_identity_mismatch"
        }
        BoundCanonicalDispatchError::MissingProviderObservation => {
            "managed_consequential_observation_authority_missing"
        }
        BoundCanonicalDispatchError::ProviderIncarnationMismatch => {
            "managed_consequential_provider_incarnation_stale"
        }
        BoundCanonicalDispatchError::TargetIncarnationMismatch => {
            "managed_consequential_target_incarnation_stale"
        }
        BoundCanonicalDispatchError::InternalCaptureActionUnsupported => {
            "managed_consequential_internal_action_rejected"
        }
        BoundCanonicalDispatchError::PublicQueueRejected => {
            "managed_consequential_dispatch_queue_rejected"
        }
    }
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn denied() -> axum::response::Response {
    error(StatusCode::UNAUTHORIZED, "unauthorized")
}

fn error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

fn lock_registry(
    registry: &'static StdMutex<ManagedConsequentialControlRegistry>,
) -> MutexGuard<'static, ManagedConsequentialControlRegistry> {
    match registry.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localview_protocol::SemanticNode;
    use std::collections::BTreeMap;

    fn node(reference: &str, interactive: bool, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            reference: reference.into(),
            role: None,
            name: None,
            tag: "div".into(),
            rect: None,
            interactive,
            attributes: BTreeMap::new(),
            source: None,
            ownership: None,
            children,
        }
    }

    #[test]
    fn managed_consequential_reference_is_bounded_and_canonical() {
        assert!(valid_reference("@eabc123"));
        assert!(!valid_reference("@save"));
        assert!(!valid_reference("@e"));
        assert!(!valid_reference("@ezz"));
    }

    #[test]
    fn managed_consequential_target_lookup_is_exact() {
        let root = node(
            "@e1",
            false,
            vec![node("@e2", true, vec![node("@e3", true, vec![])])],
        );
        assert!(find_node(&root, "@e3").is_some());
        assert!(find_node(&root, "@e4").is_none());
    }

    #[test]
    fn managed_consequential_contract_refs_are_bounded() {
        assert!(valid_postcondition_refs(
            &["lvpc:web-semantic:v1:{}".into()]
        ));
        assert!(!valid_postcondition_refs(&[]));
        assert!(!valid_postcondition_refs(&["not-a-contract".into()]));
    }
}
