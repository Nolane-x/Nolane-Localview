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
use chrono::Utc;
use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionPostconditionVerdict, ActionRiskClass,
    BoundCanonicalDispatchError, BridgeAction, BridgeActionKind, BridgeActionResult,
    CanonicalActionOperation, CanonicalQueuedAction, ConsequentialJournal,
    ConsequentialPostconditionEvidence,
    ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,
    ConsequentialRecoveryState, DispatchExecutionPermit, DispatchLinearizationReceipt,
    DispatchPreparationReceipt, DispatchPreparedCapability, LiveBridge,
    ManagedWebPayloadCommitmentKey, ManagedWebPayloadRef,
    reconcile_consequential_postconditions, verify_managed_web_payload_binding,
};
use localview_postcondition_contracts::{
    PostconditionContractRegistry, RegisteredPostconditionContract,
    WebSemanticPostconditionEvaluation,
};
use localview_protocol::{
    DispatchResult, PageSnapshot, PrincipalRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, SemanticNode, SessionId, TransportResult,
};
use localview_sessions::SessionManager;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    ControlState,
    fresh_snapshot::acquire_fresh_semantic_snapshot,
    resource_runtime::{
        ManagedSurfaceActionAuthority,
        current_primary_managed_surface_action_authority_for_sessions,
    },
};

const MAX_PENDING_MANAGED_CONSEQUENTIAL_PLANS: usize = 64;
const MAX_MANAGED_CONSEQUENTIAL_RECONCILIATIONS: usize = 128;
const MAX_POSTCONDITION_CONTRACTS: usize = 8;
const MAX_POSTCONDITION_CONTRACT_REF_BYTES: usize = 4 * 1024;
const MAX_REFERENCE_BYTES: usize = 256;
const MAX_MANAGED_TYPE_UTF8_BYTES: usize = 16 * 1024;
const MAX_MANAGED_KEY_UTF8_BYTES: usize = 256;
const MAX_MANAGED_MODIFIERS: usize = 8;
const MAX_MANAGED_MODIFIER_UTF8_BYTES: usize = 64;
const MAX_MANAGED_SCROLL_DELTA: f64 = 100_000.0;
const CONFIRMATION_TTL: Duration = Duration::from_secs(30);
const RECONCILIATION_RECORD_TTL: Duration = Duration::from_secs(5 * 60);
const MANAGED_RECONCILIATION_SURFACE_SCOPE: &str = "managed-webview:semantic";
const MANAGED_RECONCILIATION_CACHE_PROFILE_REVISION: &str =
    "cache:managed-webview:fresh-semantic:v1";
const MANAGED_RECONCILIATION_PERMISSION_VISIBILITY_REVISION: &str =
    "visibility:managed-webview:session-scoped:v1";
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

struct ProcessLocalManagedWebPayload {
    payload_ref: ManagedWebPayloadRef,
    bytes: Zeroizing<Vec<u8>>,
}

struct PendingManagedConsequentialPlan {
    confirmation_ref: Uuid,
    queued: CanonicalQueuedAction,
    payload: Option<ProcessLocalManagedWebPayload>,
    surface_authority: ManagedSurfaceActionAuthority,
    expires_at: Instant,
}

struct PreparedManagedConsequentialDispatch {
    session_id: SessionId,
    capability: DispatchPreparedCapability,
}

struct ExecutingManagedConsequentialDispatch {
    session_id: SessionId,
    permit: DispatchExecutionPermit,
}

#[derive(Debug, Clone)]
struct ManagedConsequentialReconciliation {
    session_id: SessionId,
    surface_authority: ManagedSurfaceActionAuthority,
    expected_postcondition_contract_refs: Vec<String>,
    status: &'static str,
    proof_ref: Option<String>,
    snapshot_version: Option<u64>,
    detail: Option<String>,
    expires_at: Instant,
}

#[derive(Clone)]
struct ManagedConsequentialControlHandle {
    journal: Option<Arc<ConsequentialJournal>>,
    payload_commitment_key: Option<Arc<ManagedWebPayloadCommitmentKey>>,
    pending: Arc<Mutex<HashMap<Uuid, PendingManagedConsequentialPlan>>>,
    prepared: Arc<Mutex<HashMap<Uuid, PreparedManagedConsequentialDispatch>>>,
    executing: Arc<Mutex<HashMap<Uuid, ExecutingManagedConsequentialDispatch>>>,
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

pub fn configure_managed_consequential_control_for_sessions(
    sessions: &Arc<SessionManager>,
    journal: Option<Arc<ConsequentialJournal>>,
) {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = MANAGED_CONSEQUENTIAL_CONTROL.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.insert(
        key,
        ManagedConsequentialControlEntry {
            owner: Arc::downgrade(sessions),
            handle: ManagedConsequentialControlHandle {
                journal,
                payload_commitment_key: ManagedWebPayloadCommitmentKey::generate()
                    .ok()
                    .map(Arc::new),
                pending: Arc::new(Mutex::new(HashMap::new())),
                prepared: Arc::new(Mutex::new(HashMap::new())),
                executing: Arc::new(Mutex::new(HashMap::new())),
                reconciliations: Arc::new(Mutex::new(HashMap::new())),
                plan_gate: Arc::new(Mutex::new(())),
            },
        },
    );
}

/// Configure durable history only. Process-local pending confirmations and
/// reconciliation capabilities are always recreated empty and are never
/// restored from journal contents after restart.
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
    let prepared = {
        let mut prepared = handle.prepared.lock().await;
        let action_ids = prepared
            .iter()
            .filter_map(|(action_id, entry)| (entry.session_id == session_id).then_some(*action_id))
            .collect::<Vec<_>>();
        action_ids
            .into_iter()
            .filter_map(|action_id| prepared.remove(&action_id).map(|entry| (action_id, entry)))
            .collect::<Vec<_>>()
    };
    let executing = {
        let mut executing = handle.executing.lock().await;
        let action_ids = executing
            .iter()
            .filter_map(|(action_id, entry)| (entry.session_id == session_id).then_some(*action_id))
            .collect::<Vec<_>>();
        action_ids
            .into_iter()
            .filter_map(|action_id| executing.remove(&action_id).map(|entry| (action_id, entry)))
            .collect::<Vec<_>>()
    };

    if let Some(journal) = handle.journal.as_ref() {
        for (action_id, prepared) in prepared {
            if let Ok(permit) = journal.begin_dispatch(prepared.capability).await {
                let _ = journal
                    .record_dispatch_linearized(
                        permit,
                        DispatchLinearizationReceipt {
                            receipt_ref: format!(
                                "dispatch:managed-webview:session-release:{action_id}:{}",
                                Uuid::new_v4()
                            ),
                            transport_result: TransportResult::RejectedBeforeExecutor,
                            dispatch_result: DispatchResult::NotDispatched,
                        },
                    )
                    .await;
            }
        }
        for (_, executing) in executing {
            let _ = journal.abandon_dispatch_execution(executing.permit).await;
        }
    }

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
    let ManagedConsequentialPlanRequest {
        reference,
        action,
        expected_postcondition_contract_refs,
    } = request;
    if !valid_reference(&reference) {
        return error(
            StatusCode::BAD_REQUEST,
            "managed_consequential_invalid_reference",
        );
    }
    let (carrier_action, payload, public_action) = match prepare_managed_action(action) {
        Ok(prepared) => prepared,
        Err(code) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": code,
                    "supported": ["click", "focus", "type_text", "key", "scroll"],
                    "dispatch_performed": false,
                    "confirmation_created": false,
                })),
            )
                .into_response();
        }
    };
    if !valid_postcondition_refs(&expected_postcondition_contract_refs) {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "managed_consequential_invalid_postcondition_contract",
        );
    }

    let control = control_for_sessions(&state.sessions);
    let Some(journal) = control.journal.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "managed_consequential_durable_journal_unavailable",
        );
    };
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

    let Some(node) = find_node(&snapshot.root, &reference) else {
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
        snapshot_cut_ref(&snapshot, &reference, &authority_after)
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
            Some(reference.clone()),
            carrier_action.clone(),
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
                expected_postcondition_contract_refs: expected_postcondition_contract_refs.clone(),
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
    if journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .is_err()
    {
        state.live.discard_bound_canonical_action(action_id).await;
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "managed_consequential_durable_intent_failed",
        );
    }
    if journal
        .record_intent_operation_bound(&queued)
        .await
        .is_err()
    {
        state.live.discard_bound_canonical_action(action_id).await;
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "managed_consequential_durable_operation_failed",
        );
    }

    if let Some(payload) = payload.as_ref() {
        let Some(key) = control.payload_commitment_key.as_ref() else {
            state.live.discard_bound_canonical_action(action_id).await;
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "managed_consequential_payload_commitment_key_unavailable",
            );
        };
        if journal
            .record_managed_web_payload_binding(
                &queued,
                key.as_ref(),
                payload.payload_ref,
                payload.bytes.as_slice(),
            )
            .await
            .is_err()
        {
            state.live.discard_bound_canonical_action(action_id).await;
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "managed_consequential_payload_binding_failed",
            );
        }
    }

    control.pending.lock().await.insert(
        action_id,
        PendingManagedConsequentialPlan {
            confirmation_ref,
            queued,
            payload,
            surface_authority: authority_after.clone(),
            expires_at: Instant::now() + CONFIRMATION_TTL,
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "action_id": action_id,
            "confirmation_ref": confirmation_ref,
            "action": public_action,
            "reference": reference,
            "precondition_snapshot_cut_ref": precondition_snapshot_cut_ref,
            "provider_incarnation_ref": authority_after.provider_incarnation_ref,
            "target_incarnation_ref": authority_after.target_incarnation_ref,
            "expected_postcondition_contract_refs": expected_postcondition_contract_refs,
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
    let Some(journal) = control.journal.as_ref() else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "managed_consequential_durable_journal_unavailable",
        );
    };
    let _plan_gate = control.plan_gate.lock().await;

    let mut plan = {
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

    prune_expired_reconciliations(&control).await;
    if control.reconciliations.lock().await.len() >= MAX_MANAGED_CONSEQUENTIAL_RECONCILIATIONS {
        state
            .live
            .discard_bound_canonical_action(plan.queued.action.id)
            .await;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "managed_consequential_reconciliation_capacity_exhausted",
                "action_id": action_id,
                "confirmation_consumed": true,
                "dispatch_performed": false,
                "retry_same_confirmation_allowed": false,
                "max_reconciliation_records": MAX_MANAGED_CONSEQUENTIAL_RECONCILIATIONS,
            })),
        )
            .into_response();
    }

    if let Some(payload) = plan.payload.take() {
        match materialize_verified_managed_payload(&control, journal, action_id, payload).await {
            Ok(action) => plan.queued.action.action = action,
            Err(code) => {
                state
                    .live
                    .discard_bound_canonical_action(plan.queued.action.id)
                    .await;
                return error(StatusCode::CONFLICT, code);
            }
        }
    }

    let authorization_entry = match journal
        .record_authorization(
            action_id,
            plan.queued.envelope.metadata.authorization_revision.clone(),
            true,
        )
        .await
    {
        Ok(entry) => entry,
        Err(_) => {
            state
                .live
                .discard_bound_canonical_action(plan.queued.action.id)
                .await;
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "managed_consequential_durable_authorization_failed",
            );
        }
    };
    let preparation = DispatchPreparationReceipt {
        receipt_ref: format!(
            "dispatch-prepared:managed-webview:{action_id}:{}",
            authorization_entry.journal_sequence
        ),
        authorization_journal_sequence: authorization_entry.journal_sequence,
        precondition_snapshot_cut_ref: plan
            .queued
            .envelope
            .metadata
            .precondition_snapshot_cut_ref
            .clone(),
        provider_incarnation_ref: plan
            .queued
            .envelope
            .metadata
            .provider_incarnation_ref
            .clone(),
        target_incarnation_ref: plan.queued.envelope.metadata.target_incarnation_ref.clone(),
    };
    let prepared = match journal
        .record_dispatch_prepared(action_id, preparation)
        .await
    {
        Ok(admission) => admission,
        Err(_) => {
            state
                .live
                .discard_bound_canonical_action(plan.queued.action.id)
                .await;
            return error(
                StatusCode::SERVICE_UNAVAILABLE,
                "managed_consequential_durable_prepare_failed",
            );
        }
    };
    let (_, capability) = prepared.into_parts();
    control.prepared.lock().await.insert(
        action_id,
        PreparedManagedConsequentialDispatch {
            session_id,
            capability,
        },
    );

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
            let rollback_failed = match control.prepared.lock().await.remove(&action_id) {
                Some(prepared) => match journal.begin_dispatch(prepared.capability).await {
                    Ok(permit) => journal
                        .record_dispatch_linearized(
                            permit,
                            DispatchLinearizationReceipt {
                                receipt_ref: format!(
                                    "dispatch:managed-webview:not-dispatched:{action_id}:{}",
                                    Uuid::new_v4()
                                ),
                                transport_result: TransportResult::RejectedBeforeExecutor,
                                dispatch_result: DispatchResult::NotDispatched,
                            },
                        )
                        .await
                        .is_err(),
                    Err(_) => true,
                },
                None => true,
            };
            (
                if rollback_failed {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::CONFLICT
                },
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
        return error(
            StatusCode::NOT_FOUND,
            "managed_consequential_status_not_found",
        );
    };
    let record = {
        let mut records = control.reconciliations.lock().await;
        match records.get(&action_id) {
            Some(record) => {
                if record.session_id != session_id {
                    return error(
                        StatusCode::CONFLICT,
                        "managed_consequential_status_session_mismatch",
                    );
                }
                if record.expires_at <= Instant::now() {
                    records.remove(&action_id);
                    None
                } else {
                    Some(record.clone())
                }
            }
            None => None,
        }
    };
    if let Some(record) = record {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "action_id": action_id,
                "postcondition_status": record.status,
                "expected_postcondition_contract_refs": record.expected_postcondition_contract_refs,
                "proof_ref": record.proof_ref,
                "fresh_snapshot_version": record.snapshot_version,
                "detail": record.detail,
                "terminal": !matches!(
                    record.status,
                    "pending_executor_completion" | "pending_fresh_reconciliation"
                ),
                "durable_recovery": false,
            })),
        )
            .into_response();
    }

    durable_managed_consequential_status(&control, session_id, action_id).await
}

async fn durable_managed_consequential_status(
    control: &ManagedConsequentialControlHandle,
    session_id: SessionId,
    action_id: Uuid,
) -> axum::response::Response {
    let Some(journal) = control.journal.as_ref() else {
        return error(
            StatusCode::NOT_FOUND,
            "managed_consequential_status_not_found",
        );
    };
    let binding = journal
        .recovery_binding_inventory()
        .await
        .into_iter()
        .find(|entry| entry.action_id == action_id);
    let Some(binding) = binding else {
        return error(
            StatusCode::NOT_FOUND,
            "managed_consequential_status_not_found",
        );
    };
    if binding.session_id != session_id {
        return error(
            StatusCode::CONFLICT,
            "managed_consequential_status_session_mismatch",
        );
    }
    if !binding
        .provider_incarnation_ref
        .as_str()
        .starts_with("provider:managed-webview:")
        || !binding
            .target_incarnation_ref
            .as_str()
            .starts_with("target:managed-webview:")
    {
        return error(
            StatusCode::NOT_FOUND,
            "managed_consequential_status_not_found",
        );
    }

    let receipt = journal.latest_action_postcondition_receipt(action_id).await;
    let proof_ref = receipt.as_ref().map(|receipt| receipt.receipt_ref.clone());
    let (postcondition_status, detail) = match binding.recovery_state {
        ConsequentialRecoveryState::Admitted
        | ConsequentialRecoveryState::AuthorizedNotDispatched
        | ConsequentialRecoveryState::KnownNotDispatched => (
            "known_not_dispatched",
            "durable_recovery_proves_no_dispatch_authority",
        ),
        ConsequentialRecoveryState::DispatchPrepared
        | ConsequentialRecoveryState::PossiblyDispatched => (
            "reconciliation_required",
            "durable_recovery_requires_original_post_dispatch_lineage",
        ),
        ConsequentialRecoveryState::OutcomeObservedUnverified => {
            match receipt.as_ref().map(|receipt| receipt.verdict) {
                Some(ActionPostconditionVerdict::VerifiedUnexpected) => (
                    "verified_unexpected",
                    "durable_postcondition_receipt_observed_unexpected_world_state",
                ),
                _ => (
                    "reconciliation_required",
                    "durable_postcondition_receipt_requires_reconciliation",
                ),
            }
        }
        ConsequentialRecoveryState::VerifiedUncommitted => (
            "reconciliation_required",
            "durable_verified_receipt_commit_pending",
        ),
        ConsequentialRecoveryState::Committed => {
            ("verified_expected", "durable_verified_receipt_committed")
        }
        ConsequentialRecoveryState::Compensated => (
            "reconciliation_required",
            "durable_action_was_compensated_outside_managed_webview_scope",
        ),
        ConsequentialRecoveryState::CompensationFailed => {
            ("reconciliation_required", "durable_compensation_failed")
        }
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "action_id": action_id,
            "postcondition_status": postcondition_status,
            "expected_postcondition_contract_refs": binding.expected_postcondition_contract_refs,
            "proof_ref": proof_ref,
            "fresh_snapshot_version": serde_json::Value::Null,
            "detail": detail,
            "terminal": true,
            "durable_recovery": true,
            "durable_recovery_state": durable_recovery_state_name(binding.recovery_state),
            "retry_same_confirmation_allowed": false,
        })),
    )
        .into_response()
}

fn durable_recovery_state_name(state: ConsequentialRecoveryState) -> &'static str {
    match state {
        ConsequentialRecoveryState::Admitted => "admitted",
        ConsequentialRecoveryState::AuthorizedNotDispatched => "authorized_not_dispatched",
        ConsequentialRecoveryState::DispatchPrepared => "dispatch_prepared",
        ConsequentialRecoveryState::KnownNotDispatched => "known_not_dispatched",
        ConsequentialRecoveryState::PossiblyDispatched => "possibly_dispatched",
        ConsequentialRecoveryState::OutcomeObservedUnverified => "outcome_observed_unverified",
        ConsequentialRecoveryState::VerifiedUncommitted => "verified_uncommitted",
        ConsequentialRecoveryState::Compensated => "compensated",
        ConsequentialRecoveryState::CompensationFailed => "compensation_failed",
        ConsequentialRecoveryState::Committed => "committed",
    }
}

pub(crate) async fn arm_managed_consequential_actions_for_executor(
    sessions: &Arc<SessionManager>,
    live: &LiveBridge,
    session_id: SessionId,
    authority_ref: &str,
    actions: Vec<BridgeAction>,
) -> Vec<BridgeAction> {
    let Some(control) = existing_control_for_sessions(sessions) else {
        return actions;
    };
    let Some(journal) = control.journal.as_ref() else {
        return actions;
    };

    let tracked_ids = {
        let records = control.reconciliations.lock().await;
        actions
            .iter()
            .filter(|action| {
                records
                    .get(&action.id)
                    .is_some_and(|record| record.session_id == session_id)
            })
            .map(|action| action.id)
            .collect::<HashSet<_>>()
    };
    if tracked_ids.is_empty() {
        return actions;
    }

    let mut ready = Vec::with_capacity(actions.len());
    for action in actions {
        if !tracked_ids.contains(&action.id) {
            ready.push(action);
            continue;
        }

        if verify_managed_payload_at_dispatch(&control, journal, &action)
            .await
            .is_err()
        {
            reject_managed_action_before_executor(
                &control,
                journal,
                live,
                session_id,
                authority_ref,
                action.id,
                "managed payload commitment verification failed",
            )
            .await;
            continue;
        }

        let prepared = control.prepared.lock().await.remove(&action.id);
        let permit = match prepared {
            Some(prepared) if prepared.session_id == session_id => {
                journal.begin_dispatch(prepared.capability).await.ok()
            }
            _ => None,
        };
        let Some(permit) = permit else {
            if let Some(record) = control.reconciliations.lock().await.get_mut(&action.id) {
                record.status = "durable_dispatch_authority_failed";
                record.detail = Some("journal execution permit unavailable".into());
                record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
            }
            let _ = live
                .complete_managed_surface_action(
                    session_id,
                    authority_ref,
                    BridgeActionResult {
                        action_id: action.id,
                        ok: false,
                        error: Some("durable dispatch authority unavailable".into()),
                        payload: serde_json::Value::Null,
                        completed_at: Utc::now(),
                    },
                )
                .await;
            continue;
        };

        control.executing.lock().await.insert(
            action.id,
            ExecutingManagedConsequentialDispatch { session_id, permit },
        );
        ready.push(action);
    }
    ready
}

pub(crate) async fn record_managed_consequential_executor_completion(
    sessions: &Arc<SessionManager>,
    session_id: SessionId,
    result: &BridgeActionResult,
) -> Result<bool, &'static str> {
    let Some(control) = existing_control_for_sessions(sessions) else {
        return Ok(false);
    };
    let tracked = control
        .reconciliations
        .lock()
        .await
        .get(&result.action_id)
        .is_some_and(|record| record.session_id == session_id);
    if !tracked {
        return Ok(false);
    }
    let Some(journal) = control.journal.as_ref() else {
        return Err("managed_consequential_durable_journal_unavailable");
    };
    let Some(executing) = control.executing.lock().await.remove(&result.action_id) else {
        return Err("managed_consequential_dispatch_permit_missing");
    };
    if executing.session_id != session_id {
        return Err("managed_consequential_dispatch_session_mismatch");
    }

    let dispatch_result = if result.ok {
        DispatchResult::DispatchedFull
    } else {
        // A negative executor acknowledgement does not prove that no side effect
        // crossed the page boundary. Preserve uncertainty and reconcile fresh
        // world state instead of manufacturing known-not-dispatched evidence.
        DispatchResult::DispatchAmbiguous
    };
    journal
        .record_dispatch_linearized(
            executing.permit,
            DispatchLinearizationReceipt {
                receipt_ref: format!(
                    "dispatch:managed-webview:{}:{}",
                    result.action_id,
                    Uuid::new_v4()
                ),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result,
            },
        )
        .await
        .map_err(|_| "managed_consequential_durable_dispatch_receipt_failed")?;
    Ok(true)
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
            record.status = "pending_fresh_reconciliation";
            record.detail =
                (!result.ok).then(|| "executor_reported_failure_dispatch_ambiguous".to_owned());
            record.clone()
        };

        let Some(journal) = control.journal.as_ref() else {
            set_reconciliation_required(
                &control,
                action_id,
                "managed_consequential_durable_journal_unavailable",
            )
            .await;
            return;
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

        let observation_matches =
            state
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

        let observation_permit = match journal.begin_postcondition_observation(action_id).await {
            Ok(permit) => permit,
            Err(_) => {
                set_reconciliation_required(
                    &control,
                    action_id,
                    "durable_post_dispatch_observation_authority_unavailable",
                )
                .await;
                return;
            }
        };
        let post_dispatch_cut_ref = observation_permit.snapshot_cut_ref().to_owned();

        let snapshot = match acquire_fresh_semantic_snapshot(&state, session_id).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                let _ = journal
                    .abandon_postcondition_observation(observation_permit)
                    .await;
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
            let _ = journal
                .abandon_postcondition_observation(observation_permit)
                .await;
            set_reconciliation_required(
                &control,
                action_id,
                "managed_surface_authority_changed_during_reconciliation",
            )
            .await;
            return;
        }
        let observation_still_matches = state
            .live
            .observation_status(session_id)
            .await
            .is_some_and(|status| {
                status.provider_incarnation_ref
                    == initial.surface_authority.provider_incarnation_ref
                    && status.target_incarnation_ref
                        == initial.surface_authority.target_incarnation_ref
            });
        if !observation_still_matches {
            let _ = journal
                .abandon_postcondition_observation(observation_permit)
                .await;
            set_reconciliation_required(
                &control,
                action_id,
                "managed_observation_lineage_changed_during_reconciliation",
            )
            .await;
            return;
        }

        let Some(observed_digest) = semantic_snapshot_digest(&snapshot) else {
            let _ = journal
                .abandon_postcondition_observation(observation_permit)
                .await;
            set_reconciliation_required(
                &control,
                action_id,
                "fresh_post_dispatch_snapshot_digest_failed",
            )
            .await;
            return;
        };
        let reconciliation_snapshot = ReconciliationSnapshotReceipt {
            receipt_id: format!("reconcile:managed-webview:{action_id}:{}", Uuid::new_v4()),
            provider_incarnation_ref: initial.surface_authority.provider_incarnation_ref.clone(),
            target_incarnation_ref: initial.surface_authority.target_incarnation_ref.clone(),
            snapshot_cut_ref: post_dispatch_cut_ref,
            surface_scope: MANAGED_RECONCILIATION_SURFACE_SCOPE.to_owned(),
            completeness: ReconciliationCompleteness::Established,
            cache_profile_revision: MANAGED_RECONCILIATION_CACHE_PROFILE_REVISION.to_owned(),
            permission_visibility_revision: MANAGED_RECONCILIATION_PERMISSION_VISIBILITY_REVISION
                .to_owned(),
            capture_sequence: snapshot.version,
            observed_digest: observed_digest.clone(),
            incompleteness_debt: Vec::new(),
        };

        if !state
            .live
            .record_reconciliation(session_id, reconciliation_snapshot.clone())
            .await
        {
            let _ = journal
                .abandon_postcondition_observation(observation_permit)
                .await;
            set_reconciliation_required(
                &control,
                action_id,
                "managed_reconciliation_snapshot_rejected",
            )
            .await;
            return;
        }

        let observation_receipt = match journal
            .complete_postcondition_observation(observation_permit, reconciliation_snapshot)
            .await
        {
            Ok(receipt) => receipt,
            Err(_) => {
                set_reconciliation_required(
                    &control,
                    action_id,
                    "durable_post_dispatch_observation_completion_failed",
                )
                .await;
                return;
            }
        };

        let registry = PostconditionContractRegistry::standard();
        let mut evidence = Vec::with_capacity(initial.expected_postcondition_contract_refs.len());
        for contract_ref in &initial.expected_postcondition_contract_refs {
            let status = match registry.evaluate_web_semantic(contract_ref, &snapshot) {
                Ok(WebSemanticPostconditionEvaluation::VerifiedPass) => {
                    ConsequentialPostconditionStatus::VerifiedPass
                }
                Ok(WebSemanticPostconditionEvaluation::VerifiedFail) => {
                    ConsequentialPostconditionStatus::VerifiedFail
                }
                Ok(WebSemanticPostconditionEvaluation::Unknown) | Err(_) => {
                    ConsequentialPostconditionStatus::Unknown
                }
            };
            evidence.push(ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status,
                receipt_ref: postcondition_evidence_receipt_ref(
                    action_id,
                    &observed_digest,
                    contract_ref,
                    status,
                ),
            });
        }

        let durable = match reconcile_consequential_postconditions(
            &state.live,
            journal,
            ConsequentialPostconditionReconciliationReceipt::from_observation(
                observation_receipt,
                evidence,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                set_reconciliation_required(
                    &control,
                    action_id,
                    "durable_postcondition_reconciliation_failed",
                )
                .await;
                return;
            }
        };

        let mut status = match durable.postcondition_receipt.verdict {
            ActionPostconditionVerdict::VerifiedExpected => "verified_expected",
            ActionPostconditionVerdict::VerifiedUnexpected => "verified_unexpected",
            ActionPostconditionVerdict::ReconciliationRequired => "reconciliation_required",
        };
        let mut durable_commit_failed = false;
        if durable.postcondition_receipt.verdict == ActionPostconditionVerdict::VerifiedExpected
            && journal.record_committed(action_id).await.is_err()
        {
            status = "reconciliation_required";
            durable_commit_failed = true;
        }

        let mut records = control.reconciliations.lock().await;
        if let Some(record) = records.get_mut(&action_id) {
            record.status = status;
            record.proof_ref = Some(durable.postcondition_receipt.receipt_ref.clone());
            record.snapshot_version = Some(snapshot.version);
            record.detail = if durable_commit_failed {
                Some("durable_verified_receipt_commit_failed".to_owned())
            } else if status == "reconciliation_required" {
                Some("one_or_more_postconditions_unresolved".to_owned())
            } else if !result.ok {
                Some("executor_reported_failure_but_world_state_reconciled".to_owned())
            } else {
                None
            };
            record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
        }
    });
}

fn semantic_snapshot_digest(snapshot: &PageSnapshot) -> Option<String> {
    let encoded = serde_json::to_vec(snapshot).ok()?;
    let mut digest = Sha256::new();
    digest.update(b"localview-managed-webview-postdispatch-snapshot-v1\0");
    digest.update(encoded);
    let digest = digest.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        write!(&mut hex, "{byte:02x}").ok()?;
    }
    Some(format!("sha256:{hex}"))
}

fn postcondition_evidence_receipt_ref(
    action_id: Uuid,
    observed_digest: &str,
    contract_ref: &str,
    status: ConsequentialPostconditionStatus,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"localview-managed-webview-postcondition-evidence-v1\0");
    digest.update(action_id.as_bytes());
    digest.update(b"\0");
    digest.update(observed_digest.as_bytes());
    digest.update(b"\0");
    digest.update(contract_ref.as_bytes());
    digest.update(b"\0");
    digest.update(match status {
        ConsequentialPostconditionStatus::VerifiedPass => b"verified_pass".as_slice(),
        ConsequentialPostconditionStatus::VerifiedFail => b"verified_fail".as_slice(),
        ConsequentialPostconditionStatus::Unknown => b"unknown".as_slice(),
    });
    let digest = digest.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("evidence:managed-webview:sha256:{hex}")
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
                journal: None,
                payload_commitment_key: ManagedWebPayloadCommitmentKey::generate()
                    .ok()
                    .map(Arc::new),
                pending: Arc::new(Mutex::new(HashMap::new())),
                prepared: Arc::new(Mutex::new(HashMap::new())),
                executing: Arc::new(Mutex::new(HashMap::new())),
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

async fn prune_expired_reconciliations(control: &ManagedConsequentialControlHandle) {
    let now = Instant::now();
    control
        .reconciliations
        .lock()
        .await
        .retain(|_, record| record.expires_at > now);
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

fn prepare_managed_action(
    action: BridgeActionKind,
) -> Result<
    (
        BridgeActionKind,
        Option<ProcessLocalManagedWebPayload>,
        serde_json::Value,
    ),
    &'static str,
> {
    match action {
        BridgeActionKind::Click => Ok((
            BridgeActionKind::Click,
            None,
            serde_json::json!({"type": "click"}),
        )),
        BridgeActionKind::Focus => Ok((
            BridgeActionKind::Focus,
            None,
            serde_json::json!({"type": "focus"}),
        )),
        BridgeActionKind::TypeText {
            mut text,
            clear_first,
        } => {
            if text.len() > MAX_MANAGED_TYPE_UTF8_BYTES || text.as_bytes().contains(&0) {
                text.zeroize();
                return Err("managed_consequential_type_payload_invalid");
            }
            let actual = BridgeActionKind::TypeText {
                text,
                clear_first,
            };
            prepare_payload_action(
                actual,
                BridgeActionKind::TypeText {
                    text: String::new(),
                    clear_first,
                },
                serde_json::json!({"type": "type_text", "clear_first": clear_first}),
            )
        }
        BridgeActionKind::Key {
            mut key,
            mut modifiers,
        } => {
            let valid = !key.is_empty()
                && key.len() <= MAX_MANAGED_KEY_UTF8_BYTES
                && !key.as_bytes().contains(&0)
                && modifiers.len() <= MAX_MANAGED_MODIFIERS
                && modifiers.iter().all(|modifier| {
                    !modifier.is_empty()
                        && modifier.len() <= MAX_MANAGED_MODIFIER_UTF8_BYTES
                        && !modifier.as_bytes().contains(&0)
                });
            if !valid {
                key.zeroize();
                for modifier in &mut modifiers {
                    modifier.zeroize();
                }
                modifiers.clear();
                return Err("managed_consequential_key_payload_invalid");
            }
            let actual = BridgeActionKind::Key { key, modifiers };
            prepare_payload_action(
                actual,
                BridgeActionKind::Key {
                    key: String::new(),
                    modifiers: Vec::new(),
                },
                serde_json::json!({"type": "key"}),
            )
        }
        BridgeActionKind::Scroll { x, y } => {
            if !x.is_finite()
                || !y.is_finite()
                || x.abs() > MAX_MANAGED_SCROLL_DELTA
                || y.abs() > MAX_MANAGED_SCROLL_DELTA
                || (x == 0.0 && y == 0.0)
            {
                return Err("managed_consequential_scroll_payload_invalid");
            }
            prepare_payload_action(
                BridgeActionKind::Scroll { x, y },
                BridgeActionKind::Scroll { x: 0.0, y: 0.0 },
                serde_json::json!({"type": "scroll"}),
            )
        }
        _ => Err("managed_consequential_action_not_supported"),
    }
}

fn prepare_payload_action(
    mut actual: BridgeActionKind,
    carrier: BridgeActionKind,
    mut public_action: serde_json::Value,
) -> Result<
    (
        BridgeActionKind,
        Option<ProcessLocalManagedWebPayload>,
        serde_json::Value,
    ),
    &'static str,
> {
    let bytes = serde_json::to_vec(&actual)
        .map(Zeroizing::new)
        .map_err(|_| "managed_consequential_payload_encoding_failed")?;
    zeroize_action_payload(&mut actual);
    let payload_ref = ManagedWebPayloadRef(Uuid::new_v4());
    if let Some(object) = public_action.as_object_mut() {
        object.insert(
            "payload_ref".into(),
            serde_json::Value::String(payload_ref.0.to_string()),
        );
    }
    Ok((
        carrier,
        Some(ProcessLocalManagedWebPayload { payload_ref, bytes }),
        public_action,
    ))
}

fn zeroize_action_payload(action: &mut BridgeActionKind) {
    match action {
        BridgeActionKind::TypeText { text, .. } => text.zeroize(),
        BridgeActionKind::Key { key, modifiers } => {
            key.zeroize();
            for modifier in modifiers.iter_mut() {
                modifier.zeroize();
            }
            modifiers.clear();
        }
        _ => {}
    }
}

async fn materialize_verified_managed_payload(
    control: &ManagedConsequentialControlHandle,
    journal: &ConsequentialJournal,
    action_id: Uuid,
    payload: ProcessLocalManagedWebPayload,
) -> Result<BridgeActionKind, &'static str> {
    let Some(key) = control.payload_commitment_key.as_ref() else {
        return Err("managed_consequential_payload_commitment_key_unavailable");
    };
    let binding = journal
        .managed_web_payload_binding(action_id)
        .await
        .map_err(|_| "managed_consequential_payload_binding_unavailable")?
        .ok_or("managed_consequential_payload_binding_missing")?;
    if binding.payload_ref != payload.payload_ref {
        return Err("managed_consequential_payload_ref_mismatch");
    }
    verify_managed_web_payload_binding(key.as_ref(), &binding, payload.bytes.as_slice())
        .map_err(|_| "managed_consequential_payload_commitment_mismatch")?;
    let action: BridgeActionKind = serde_json::from_slice(payload.bytes.as_slice())
        .map_err(|_| "managed_consequential_payload_decode_failed")?;
    if CanonicalActionOperation::from_bridge_action_kind(&action) != Some(binding.operation) {
        return Err("managed_consequential_payload_operation_mismatch");
    }
    validate_materialized_payload_action(&action)?;
    Ok(action)
}

fn validate_materialized_payload_action(action: &BridgeActionKind) -> Result<(), &'static str> {
    match action {
        BridgeActionKind::TypeText { text, .. }
            if text.len() <= MAX_MANAGED_TYPE_UTF8_BYTES && !text.as_bytes().contains(&0) =>
        {
            Ok(())
        }
        BridgeActionKind::Key { key, modifiers }
            if !key.is_empty()
                && key.len() <= MAX_MANAGED_KEY_UTF8_BYTES
                && !key.as_bytes().contains(&0)
                && modifiers.len() <= MAX_MANAGED_MODIFIERS
                && modifiers.iter().all(|modifier| {
                    !modifier.is_empty()
                        && modifier.len() <= MAX_MANAGED_MODIFIER_UTF8_BYTES
                        && !modifier.as_bytes().contains(&0)
                }) =>
        {
            Ok(())
        }
        BridgeActionKind::Scroll { x, y }
            if x.is_finite()
                && y.is_finite()
                && x.abs() <= MAX_MANAGED_SCROLL_DELTA
                && y.abs() <= MAX_MANAGED_SCROLL_DELTA
                && (*x != 0.0 || *y != 0.0) =>
        {
            Ok(())
        }
        BridgeActionKind::Click | BridgeActionKind::Focus => Ok(()),
        _ => Err("managed_consequential_payload_invalid"),
    }
}

async fn verify_managed_payload_at_dispatch(
    control: &ManagedConsequentialControlHandle,
    journal: &ConsequentialJournal,
    action: &BridgeAction,
) -> Result<(), &'static str> {
    if matches!(
        &action.action,
        BridgeActionKind::Click | BridgeActionKind::Focus
    ) {
        return Ok(());
    }
    validate_materialized_payload_action(&action.action)?;
    let Some(operation) = CanonicalActionOperation::from_bridge_action_kind(&action.action) else {
        return Err("managed_consequential_payload_operation_missing");
    };
    if !matches!(
        operation,
        CanonicalActionOperation::InputText
            | CanonicalActionOperation::KeyInput
            | CanonicalActionOperation::Scroll
    ) {
        return Err("managed_consequential_payload_operation_unsupported");
    }
    let Some(key) = control.payload_commitment_key.as_ref() else {
        return Err("managed_consequential_payload_commitment_key_unavailable");
    };
    let binding = journal
        .managed_web_payload_binding(action.id)
        .await
        .map_err(|_| "managed_consequential_payload_binding_unavailable")?
        .ok_or("managed_consequential_payload_binding_missing")?;
    if binding.operation != operation {
        return Err("managed_consequential_payload_operation_mismatch");
    }
    let bytes = serde_json::to_vec(&action.action)
        .map(Zeroizing::new)
        .map_err(|_| "managed_consequential_payload_encoding_failed")?;
    verify_managed_web_payload_binding(key.as_ref(), &binding, bytes.as_slice())
        .map_err(|_| "managed_consequential_payload_commitment_mismatch")
}

async fn reject_managed_action_before_executor(
    control: &ManagedConsequentialControlHandle,
    journal: &ConsequentialJournal,
    live: &LiveBridge,
    session_id: SessionId,
    authority_ref: &str,
    action_id: Uuid,
    detail: &'static str,
) {
    if let Some(prepared) = control.prepared.lock().await.remove(&action_id) {
        if let Ok(permit) = journal.begin_dispatch(prepared.capability).await {
            let _ = journal
                .record_dispatch_linearized(
                    permit,
                    DispatchLinearizationReceipt {
                        receipt_ref: format!(
                            "dispatch:managed-webview:rejected-before-executor:{action_id}:{}",
                            Uuid::new_v4()
                        ),
                        transport_result: TransportResult::RejectedBeforeExecutor,
                        dispatch_result: DispatchResult::NotDispatched,
                    },
                )
                .await;
        }
    }
    if let Some(record) = control.reconciliations.lock().await.get_mut(&action_id) {
        record.status = "durable_payload_authority_failed";
        record.detail = Some(detail.to_owned());
        record.expires_at = Instant::now() + RECONCILIATION_RECORD_TTL;
    }
    let _ = live
        .complete_managed_surface_action(
            session_id,
            authority_ref,
            BridgeActionResult {
                action_id,
                ok: false,
                error: Some(detail.to_owned()),
                payload: serde_json::Value::Null,
                completed_at: Utc::now(),
            },
        )
        .await;
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
        assert!(valid_postcondition_refs(&[
            r#"lvpc:web-semantic:v1:{"expectation":"present","ref":"@eabc123"}"#.into()
        ]));
        assert!(!valid_postcondition_refs(&[]));
        assert!(!valid_postcondition_refs(&["not-a-contract".into()]));
    }
}


#[cfg(test)]
mod r8_managed_payload_tests {
    use super::*;

    #[test]
    fn managed_key_and_scroll_payloads_are_sanitized_before_canonical_binding() {
        let (key_carrier, key_payload, key_public) = prepare_managed_action(
            BridgeActionKind::Key {
                key: "Enter".into(),
                modifiers: vec!["Control".into(), "Shift".into()],
            },
        )
        .expect("bounded key payload");
        assert_eq!(
            key_carrier,
            BridgeActionKind::Key {
                key: String::new(),
                modifiers: Vec::new(),
            }
        );
        assert!(key_payload.is_some());
        assert_eq!(key_public["type"], "key");
        assert!(key_public.get("key").is_none());
        assert!(key_public.get("modifiers").is_none());
        assert!(key_public["payload_ref"].as_str().is_some());

        let (scroll_carrier, scroll_payload, scroll_public) =
            prepare_managed_action(BridgeActionKind::Scroll { x: 12.5, y: -48.0 })
                .expect("bounded scroll payload");
        assert_eq!(
            scroll_carrier,
            BridgeActionKind::Scroll { x: 0.0, y: 0.0 }
        );
        assert!(scroll_payload.is_some());
        assert_eq!(scroll_public["type"], "scroll");
        assert!(scroll_public.get("x").is_none());
        assert!(scroll_public.get("y").is_none());
        assert!(scroll_public["payload_ref"].as_str().is_some());
    }

    #[test]
    fn managed_payload_validation_fails_closed_on_unbounded_or_empty_input() {
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::TypeText {
                text: "x".repeat(MAX_MANAGED_TYPE_UTF8_BYTES + 1),
                clear_first: false,
            }),
            Err("managed_consequential_type_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::TypeText {
                text: "bad\0value".into(),
                clear_first: false,
            }),
            Err("managed_consequential_type_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::Key {
                key: String::new(),
                modifiers: Vec::new(),
            }),
            Err("managed_consequential_key_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::Key {
                key: "A".into(),
                modifiers: vec!["modifier".into(); MAX_MANAGED_MODIFIERS + 1],
            }),
            Err("managed_consequential_key_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::Scroll { x: 0.0, y: 0.0 }),
            Err("managed_consequential_scroll_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::Scroll {
                x: MAX_MANAGED_SCROLL_DELTA + 1.0,
                y: 1.0,
            }),
            Err("managed_consequential_scroll_payload_invalid")
        ));
        assert!(matches!(
            prepare_managed_action(BridgeActionKind::Scroll {
                x: f64::INFINITY,
                y: 1.0,
            }),
            Err("managed_consequential_scroll_payload_invalid")
        ));
    }

    #[test]
    fn managed_type_plan_projection_never_contains_plaintext() {
        let secret = "r8-private-plan-text";
        let (carrier, payload, public) = prepare_managed_action(BridgeActionKind::TypeText {
            text: secret.into(),
            clear_first: true,
        })
        .expect("bounded TypeText payload");

        assert_eq!(
            carrier,
            BridgeActionKind::TypeText {
                text: String::new(),
                clear_first: true,
            }
        );
        assert!(payload.is_some());
        let encoded_public = public.to_string();
        assert!(!encoded_public.contains(secret));
        assert_eq!(public["type"], "type_text");
        assert_eq!(public["clear_first"], true);
        assert!(public.get("text").is_none());
        assert!(public["payload_ref"].as_str().is_some());
    }
}
