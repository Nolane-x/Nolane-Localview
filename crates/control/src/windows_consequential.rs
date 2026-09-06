#![forbid(unsafe_code)]

use std::{
    collections::{BTreeSet, HashMap},
    error::Error as StdError,
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex, MutexGuard, OnceLock, Weak,
    },
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalQueuedAction,
    ConsequentialJournal,
};
use localview_postcondition_contracts::PostconditionContractRegistry;
use localview_protocol::{PrincipalRef, ProviderElementRef, SessionId};
use localview_sessions::SessionManager;
use localview_windows_observe_runtime::{
    execute_verified_canonical_uia_action, WindowsUiaActionPreflightRequest,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaSemanticPostconditionVerifier, WindowsUiaVerifiedActionTarget,
    WindowsUiaVerifiedExecutionOutcome,
};
use localview_windows_uia_provider::{
    WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
};
use serde::Deserialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{windows_observe_runtime_for_sessions, ControlState};

const MAX_PENDING_WINDOWS_CONSEQUENTIAL_PLANS: usize = 64;
const MAX_POSTCONDITION_CONTRACTS: usize = 8;
const MAX_POSTCONDITION_CONTRACT_REF_BYTES: usize = 4 * 1024;
const DECISION_PRINCIPAL_REF: &str =
    "principal:local-control:bearer-holder-explicit-confirmation-v1";
const ACTING_PRINCIPAL_REF: &str = "principal:localview-daemon:windows-uia-v1";

#[derive(Debug, Clone)]
struct PendingWindowsConsequentialPlan {
    confirmation_ref: Uuid,
    queued: CanonicalQueuedAction,
    target: WindowsUiaVerifiedActionTarget,
}

#[derive(Clone)]
struct WindowsConsequentialControlHandle {
    journal: Arc<ConsequentialJournal>,
    pending: Arc<Mutex<HashMap<Uuid, PendingWindowsConsequentialPlan>>>,
    plan_gate: Arc<Mutex<()>>,
}

struct WindowsConsequentialControlEntry {
    owner: Weak<SessionManager>,
    handle: WindowsConsequentialControlHandle,
}

type WindowsConsequentialControlRegistry = HashMap<usize, WindowsConsequentialControlEntry>;

static WINDOWS_CONSEQUENTIAL_CONTROL: OnceLock<StdMutex<WindowsConsequentialControlRegistry>> =
    OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsConsequentialInvokePlanRequest {
    element_ref: ProviderElementRef,
    expected_postcondition_contract_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsConsequentialConfirmRequest {
    confirmation_ref: Uuid,
}

#[derive(Debug)]
struct ProcessLocalConfirmationError(&'static str);

impl fmt::Display for ProcessLocalConfirmationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl StdError for ProcessLocalConfirmationError {}

/// Independent, one-shot authorization authority created only after the exact
/// process-local confirmation capability has been consumed.
///
/// The raw confirmation capability is intentionally absent from this object and
/// from the durable journal. The admitted authorization revision is a distinct
/// opaque reference, so durable replay cannot reconstruct confirmation authority.
struct ProcessLocalConfirmationAuthority {
    action_id: Uuid,
    approved_authority: ActionEnvelopeMetadata,
    used: AtomicBool,
}

impl ProcessLocalConfirmationAuthority {
    fn new(plan: &PendingWindowsConsequentialPlan) -> Self {
        Self {
            action_id: plan.queued.action.id,
            approved_authority: plan.queued.envelope.metadata.clone(),
            used: AtomicBool::new(false),
        }
    }
}

impl WindowsUiaAuthorizationRevalidator for ProcessLocalConfirmationAuthority {
    type Error = ProcessLocalConfirmationError;

    fn revalidate(
        &self,
        action_id: Uuid,
        authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
        if self.used.swap(true, Ordering::AcqRel) {
            return Err(ProcessLocalConfirmationError(
                "process-local confirmation authority was already consumed",
            ));
        }
        if action_id != self.action_id || authority != &self.approved_authority {
            return Err(ProcessLocalConfirmationError(
                "process-local confirmation does not match the exact canonical action authority",
            ));
        }

        Ok(WindowsUiaAuthorizationRevalidationReceipt {
            action_id,
            decision_principal_ref: authority.decision_principal_ref.clone(),
            acting_principal_ref: authority.acting_principal_ref.clone(),
            authorization_revision: authority.authorization_revision.clone(),
        })
    }
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/windows-observe/consequential/invoke/plan",
            post(plan_windows_consequential_invoke),
        )
        .route(
            "/v1/sessions/{id}/windows-observe/consequential/{action_id}/confirm",
            post(confirm_windows_consequential_action),
        )
        .with_state(state)
}

/// Configure the durable journal used by the production two-phase Windows
/// consequential control path for one exact SessionManager lifetime.
///
/// Every configuration creates a fresh process-local pending-confirmation set.
/// Reopening a durable journal after restart therefore restores history only,
/// never confirmation or dispatch authority.
pub fn configure_windows_consequential_control_for_sessions(
    sessions: &Arc<SessionManager>,
    journal: Option<Arc<ConsequentialJournal>>,
) {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = WINDOWS_CONSEQUENTIAL_CONTROL.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    match journal {
        Some(journal) => {
            entries.insert(
                key,
                WindowsConsequentialControlEntry {
                    owner: Arc::downgrade(sessions),
                    handle: WindowsConsequentialControlHandle {
                        journal,
                        pending: Arc::new(Mutex::new(HashMap::new())),
                        plan_gate: Arc::new(Mutex::new(())),
                    },
                },
            );
        }
        None => {
            entries.remove(&key);
        }
    }
}

/// Invalidate every unconsumed process-local confirmation for one session.
/// This is called on explicit Windows detach and on session removal.
pub async fn release_windows_consequential_control_session_for_sessions(
    sessions: &Arc<SessionManager>,
    session_id: SessionId,
) {
    let Some(handle) = windows_consequential_control_for_sessions(sessions) else {
        return;
    };
    let _plan_gate = handle.plan_gate.lock().await;
    handle
        .pending
        .lock()
        .await
        .retain(|_, plan| plan.queued.action.session_id != session_id);
}

fn windows_consequential_control_for_sessions(
    sessions: &Arc<SessionManager>,
) -> Option<WindowsConsequentialControlHandle> {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = WINDOWS_CONSEQUENTIAL_CONTROL.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get(&key).map(|entry| entry.handle.clone())
}

async fn plan_windows_consequential_invoke(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(session_id): Path<SessionId>,
    Json(request): Json<WindowsConsequentialInvokePlanRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(session_id).await.is_none() {
        return session_not_found();
    }
    if let Err(message) = validate_postcondition_contracts(
        &request.expected_postcondition_contract_refs,
    ) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "invalid_postcondition_contract",
                "message": message,
            })),
        )
            .into_response();
    }

    let Some(runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable("Windows UIA runtime is unavailable");
    };
    let Some(control) = windows_consequential_control_for_sessions(&state.sessions) else {
        return unavailable("durable consequential control journal is unavailable");
    };

    // Serialize planning so pending-confirmation capacity, fresh observation,
    // canonical admission, and session invalidation form one process-local order.
    let _plan_gate = control.plan_gate.lock().await;
    if control.pending.lock().await.len() >= MAX_PENDING_WINDOWS_CONSEQUENTIAL_PLANS {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "windows_consequential_plan_capacity_exhausted",
                "max_pending_plans": MAX_PENDING_WINDOWS_CONSEQUENTIAL_PLANS,
            })),
        )
            .into_response();
    }

    // Consequential planning never admits authority from the cached request cut.
    // The runtime first captures a fresh provider revision, publishes that exact
    // world evidence, and rebinds the requested opaque provider identity onto
    // the fresh cut. Missing, ambiguous, stale, or incomplete evidence fails
    // closed before any confirmation or durable action intent can be created.
    let fresh_evidence = match runtime
        .refresh_uia_action_evidence(session_id, request.element_ref)
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "windows_consequential_fresh_evidence_rejected",
                    "message": error.to_string(),
                    "dispatch_performed": false,
                    "confirmation_created": false,
                })),
            )
                .into_response();
        }
    };

    let refreshed_element_ref = fresh_evidence.refreshed_element_ref;

    // Confirmation and durable authorization revision are deliberately distinct
    // random values. Only the confirmation_ref is returned to the bearer holder
    // and retained process-locally; it is never written to the durable journal.
    let confirmation_ref = Uuid::new_v4();
    let authorization_revision_ref = Uuid::new_v4();
    let authority = ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from(DECISION_PRINCIPAL_REF),
        acting_principal_ref: PrincipalRef::from(ACTING_PRINCIPAL_REF),
        authorization_revision: format!(
            "authorization:local-control:confirmation-v1:{authorization_revision_ref}"
        ),
        precondition_snapshot_cut_ref: fresh_evidence.snapshot_cut_ref.clone(),
        provider_incarnation_ref: refreshed_element_ref.provider_incarnation_ref.clone(),
        target_incarnation_ref: refreshed_element_ref.target_incarnation_ref.clone(),
        risk_class: ActionRiskClass::DestructiveOrIrreversible,
        idempotency_class: ActionIdempotencyClass::Irreversible,
        expected_postcondition_contract_refs: request
            .expected_postcondition_contract_refs
            .clone(),
    };

    // Plan-time preflight remains evidence-only. It now evaluates the exact
    // refreshed element at the exact fresh cut; the coordinator repeats this gate
    // at confirmation time and later binds a fresh worker-owned dispatch lease.
    let preflight = match runtime
        .preflight_uia_action(
            session_id,
            WindowsUiaActionPreflightRequest {
                authority: authority.clone(),
                element_ref: refreshed_element_ref,
                required_pattern: WindowsUiaPattern::Invoke,
            },
        )
        .await
    {
        Ok(preflight) => preflight,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "windows_consequential_preflight_rejected",
                    "message": error.to_string(),
                    "dispatch_performed": false,
                    "confirmation_created": false,
                })),
            )
                .into_response();
        }
    };

    let queued = match state
        .live
        .bind_direct_canonical_action(
            session_id,
            Some(preflight.element_ref.opaque_provider_element_id.clone()),
            localview_live_bridge::BridgeActionKind::Click,
            authority.clone(),
        )
        .await
    {
        Ok(queued) => queued,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "windows_consequential_canonical_binding_rejected",
                    "binding_error": format!("{error:?}"),
                    "dispatch_performed": false,
                    "confirmation_created": false,
                })),
            )
                .into_response();
        }
    };

    if let Err(error) = control
        .journal
        .record_intent_admitted(queued.envelope.clone())
        .await
    {
        return durable_admission_failure(
            queued.action.id,
            "intent_admission_failed",
            error.to_string(),
        );
    }
    if let Err(error) = control.journal.record_intent_operation_bound(&queued).await {
        return durable_admission_failure(
            queued.action.id,
            "operation_binding_failed",
            error.to_string(),
        );
    }

    let target = WindowsUiaVerifiedActionTarget {
        element_ref: preflight.element_ref,
        required_pattern: WindowsUiaPattern::Invoke,
        context_requirements: WindowsUiaDispatchContextRequirements {
            require_foreground_target: true,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        },
    };
    let action_id = queued.action.id;
    control.pending.lock().await.insert(
        action_id,
        PendingWindowsConsequentialPlan {
            confirmation_ref,
            queued,
            target,
        },
    );

    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "action_id": action_id,
            "confirmation_ref": confirmation_ref,
            "confirmation_required": true,
            "confirmation_authority": "bearer_holder_explicit_confirmation",
            "operation": "invoke",
            "risk_class": "s4_destructive_or_irreversible",
            "idempotency_class": "irreversible",
            "precondition_snapshot_cut_ref": authority.precondition_snapshot_cut_ref,
            "planning_reconciliation_receipt_ref": fresh_evidence.reconciliation_receipt_ref,
            "expected_postcondition_contract_refs": authority.expected_postcondition_contract_refs,
            "restart_restores_confirmation_authority": false,
        })),
    )
        .into_response()
}

async fn confirm_windows_consequential_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((session_id, action_id)): Path<(SessionId, Uuid)>,
    Json(request): Json<WindowsConsequentialConfirmRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(session_id).await.is_none() {
        return session_not_found();
    }
    let Some(runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable("Windows UIA runtime is unavailable");
    };
    let Some(control) = windows_consequential_control_for_sessions(&state.sessions) else {
        return unavailable("durable consequential control journal is unavailable");
    };

    // Peek without consuming so a purely read-only executor-resolution failure
    // cannot burn confirmation authority. Exact consumption happens immediately
    // before the verified coordinator call.
    let Some(peeked) = peek_pending_plan(
        &control,
        session_id,
        action_id,
        request.confirmation_ref,
    )
    .await
    else {
        return confirmation_missing_or_consumed();
    };

    let executor = match runtime.uia_dispatch_executor(session_id).await {
        Ok(executor) => executor,
        Err(error) => {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "windows_consequential_executor_unavailable",
                    "message": error.to_string(),
                    "dispatch_performed": false,
                    "confirmation_consumed": false,
                })),
            )
                .into_response();
        }
    };

    let Some(plan) = consume_pending_plan(
        &control,
        session_id,
        action_id,
        request.confirmation_ref,
    )
    .await
    else {
        return confirmation_missing_or_consumed();
    };
    debug_assert_eq!(peeked.queued.action.id, plan.queued.action.id);

    let revalidator = ProcessLocalConfirmationAuthority::new(&plan);
    let verifier = WindowsUiaSemanticPostconditionVerifier;
    match execute_verified_canonical_uia_action(
        &state.live,
        control.journal.as_ref(),
        runtime.as_ref(),
        &plan.queued,
        plan.target,
        &revalidator,
        &executor,
        &verifier,
    )
    .await
    {
        Ok(outcome) => verified_outcome_response(outcome),
        Err(error) => {
            let reconciliation_required = control
                .journal
                .requires_reconciliation(action_id)
                .await
                .unwrap_or(false);
            (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "windows_consequential_verified_execution_failed",
                    "message": error.to_string(),
                    "action_id": action_id,
                    "confirmation_consumed": true,
                    "retry_allowed": false,
                    "replan_required": !reconciliation_required,
                    "reconciliation_required": reconciliation_required,
                })),
            )
                .into_response()
        }
    }
}

async fn peek_pending_plan(
    control: &WindowsConsequentialControlHandle,
    session_id: SessionId,
    action_id: Uuid,
    confirmation_ref: Uuid,
) -> Option<PendingWindowsConsequentialPlan> {
    control
        .pending
        .lock()
        .await
        .get(&action_id)
        .filter(|plan| {
            plan.queued.action.session_id == session_id
                && plan.confirmation_ref == confirmation_ref
        })
        .cloned()
}

async fn consume_pending_plan(
    control: &WindowsConsequentialControlHandle,
    session_id: SessionId,
    action_id: Uuid,
    confirmation_ref: Uuid,
) -> Option<PendingWindowsConsequentialPlan> {
    let mut pending = control.pending.lock().await;
    let exact = pending.get(&action_id).is_some_and(|plan| {
        plan.queued.action.session_id == session_id && plan.confirmation_ref == confirmation_ref
    });
    if !exact {
        return None;
    }
    pending.remove(&action_id)
}

fn validate_postcondition_contracts(contract_refs: &[String]) -> Result<(), String> {
    if contract_refs.is_empty() {
        return Err("at least one typed postcondition contract is required".into());
    }
    if contract_refs.len() > MAX_POSTCONDITION_CONTRACTS {
        return Err(format!(
            "at most {MAX_POSTCONDITION_CONTRACTS} postcondition contracts are allowed"
        ));
    }

    let registry = PostconditionContractRegistry::standard();
    let mut unique = BTreeSet::new();
    for contract_ref in contract_refs {
        if contract_ref.len() > MAX_POSTCONDITION_CONTRACT_REF_BYTES {
            return Err(format!(
                "postcondition contract exceeds {MAX_POSTCONDITION_CONTRACT_REF_BYTES} bytes"
            ));
        }
        if !unique.insert(contract_ref.as_str()) {
            return Err("duplicate postcondition contracts are not allowed".into());
        }
        registry
            .decode(contract_ref)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn durable_admission_failure(
    action_id: Uuid,
    phase: &'static str,
    message: String,
) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": "windows_consequential_durable_admission_failed",
            "phase": phase,
            "message": message,
            "action_id": action_id,
            "dispatch_performed": false,
            "confirmation_created": false,
            "retry_same_action_allowed": false,
        })),
    )
        .into_response()
}

fn verified_outcome_response(
    outcome: WindowsUiaVerifiedExecutionOutcome,
) -> axum::response::Response {
    match outcome {
        WindowsUiaVerifiedExecutionOutcome::Committed {
            action_id,
            world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
            commit_journal_sequence,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "committed",
                "action_id": action_id,
                "world_outcome": world_outcome,
                "dispatch_journal_sequence": dispatch_journal_sequence,
                "reconciliation_journal_sequence": reconciliation_journal_sequence,
                "commit_journal_sequence": commit_journal_sequence,
                "confirmation_consumed": true,
                "retry_allowed": false,
            })),
        )
            .into_response(),
        WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            action_id,
            dispatch_result,
            dispatch_journal_sequence,
        } => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "known_not_dispatched",
                "action_id": action_id,
                "dispatch_result": dispatch_result,
                "dispatch_journal_sequence": dispatch_journal_sequence,
                "confirmation_consumed": true,
                "retry_allowed": false,
                "replan_required": true,
            })),
        )
            .into_response(),
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            action_id,
            world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
        } => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "status": "postcondition_not_verified",
                "action_id": action_id,
                "world_outcome": world_outcome,
                "dispatch_journal_sequence": dispatch_journal_sequence,
                "reconciliation_journal_sequence": reconciliation_journal_sequence,
                "confirmation_consumed": true,
                "retry_allowed": false,
                "reconciliation_required": true,
            })),
        )
            .into_response(),
    }
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn denied() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
        .into_response()
}

fn session_not_found() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "session_not_found"})),
    )
        .into_response()
}

fn unavailable(message: &'static str) -> axum::response::Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "windows_consequential_control_unavailable",
            "message": message,
        })),
    )
        .into_response()
}

fn confirmation_missing_or_consumed() -> axum::response::Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "error": "windows_consequential_confirmation_missing_or_consumed",
            "retry_allowed": false,
            "replan_required": true,
        })),
    )
        .into_response()
}

fn lock_registry(
    registry: &StdMutex<WindowsConsequentialControlRegistry>,
) -> MutexGuard<'_, WindowsConsequentialControlRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use localview_live_bridge::{
        ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    };
    use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

    fn authority() -> ActionEnvelopeMetadata {
        ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from(DECISION_PRINCIPAL_REF),
            acting_principal_ref: PrincipalRef::from(ACTING_PRINCIPAL_REF),
            authorization_revision: "authorization:test:1".into(),
            precondition_snapshot_cut_ref: "cut:test:1".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:test:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:test:1"),
            risk_class: ActionRiskClass::DestructiveOrIrreversible,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![
                "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"name\":\"Done\"}}".into(),
            ],
        }
    }

    fn pending_plan() -> PendingWindowsConsequentialPlan {
        let action_id = Uuid::from_u128(0x91c0);
        let session_id = Uuid::from_u128(0x91c1);
        let authority = authority();
        PendingWindowsConsequentialPlan {
            confirmation_ref: Uuid::from_u128(0x91c2),
            queued: CanonicalQueuedAction {
                action: localview_live_bridge::BridgeAction {
                    id: action_id,
                    session_id,
                    reference: Some("uia-runtime:[1]".into()),
                    action: localview_live_bridge::BridgeActionKind::Click,
                    created_at: chrono::Utc::now(),
                },
                envelope: CanonicalActionEnvelope {
                    envelope_id: Uuid::from_u128(0x91c3),
                    transport_action_id: action_id,
                    session_id,
                    metadata: authority,
                },
            },
            target: WindowsUiaVerifiedActionTarget {
                element_ref: ProviderElementRef {
                    provider_family: "windows_uia".into(),
                    provider_incarnation_ref: ProviderIncarnationRef::from("provider:test:1"),
                    target_incarnation_ref: TargetIncarnationRef::from("target:test:1"),
                    opaque_provider_element_id: "uia-runtime:[1]".into(),
                    semantic_locator_hints: Vec::new(),
                    parent_surface_ref: None,
                    acquisition_cut_ref: "cut:test:1".into(),
                    realization: localview_protocol::ProviderElementRealization::RealizedCurrent,
                    lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
                },
                required_pattern: WindowsUiaPattern::Invoke,
                context_requirements: WindowsUiaDispatchContextRequirements {
                    require_foreground_target: true,
                    require_exact_element_focus: false,
                    require_no_modal_blocker: true,
                },
            },
        }
    }

    fn journal_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "localview-windows-consequential-control-{label}-{}.jsonl",
            Uuid::new_v4()
        ))
    }

    async fn control_handle(label: &str) -> (WindowsConsequentialControlHandle, std::path::PathBuf) {
        let path = journal_path(label);
        let journal = Arc::new(ConsequentialJournal::open(&path).await.unwrap());
        (
            WindowsConsequentialControlHandle {
                journal,
                pending: Arc::new(Mutex::new(HashMap::new())),
                plan_gate: Arc::new(Mutex::new(())),
            },
            path,
        )
    }

    #[test]
    fn decision_principal_names_only_the_authority_actually_proven() {
        assert!(DECISION_PRINCIPAL_REF.contains("bearer-holder"));
        assert!(!DECISION_PRINCIPAL_REF.contains("user"));
    }

    #[test]
    fn process_local_confirmation_is_exact_and_one_shot() {
        let plan = pending_plan();
        let revalidator = ProcessLocalConfirmationAuthority::new(&plan);
        let action_id = plan.queued.action.id;
        let authority = plan.queued.envelope.metadata.clone();

        let receipt = revalidator.revalidate(action_id, &authority).unwrap();
        assert_eq!(receipt.action_id, action_id);
        assert_eq!(receipt.authorization_revision, authority.authorization_revision);
        assert!(revalidator.revalidate(action_id, &authority).is_err());
    }

    #[test]
    fn process_local_confirmation_rejects_authority_substitution() {
        let plan = pending_plan();
        let revalidator = ProcessLocalConfirmationAuthority::new(&plan);
        let mut substituted = plan.queued.envelope.metadata.clone();
        substituted.risk_class = ActionRiskClass::ReversibleUiState;

        assert!(
            revalidator
                .revalidate(plan.queued.action.id, &substituted)
                .is_err()
        );
    }

    #[tokio::test]
    async fn wrong_confirmation_does_not_consume_exact_confirmation_and_exact_is_one_shot() {
        let (control, path) = control_handle("one-shot").await;
        let plan = pending_plan();
        let action_id = plan.queued.action.id;
        let session_id = plan.queued.action.session_id;
        let confirmation_ref = plan.confirmation_ref;
        control.pending.lock().await.insert(action_id, plan);

        assert!(
            consume_pending_plan(
                &control,
                session_id,
                action_id,
                Uuid::from_u128(0xdead),
            )
            .await
            .is_none()
        );
        assert!(
            peek_pending_plan(&control, session_id, action_id, confirmation_ref)
                .await
                .is_some(),
            "wrong confirmation must not burn the exact pending authority"
        );
        assert!(
            consume_pending_plan(&control, session_id, action_id, confirmation_ref)
                .await
                .is_some()
        );
        assert!(
            consume_pending_plan(&control, session_id, action_id, confirmation_ref)
                .await
                .is_none(),
            "exact confirmation must be consumable only once"
        );

        drop(control);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn reconfiguration_with_same_durable_journal_restores_no_pending_confirmation() {
        let sessions = Arc::new(SessionManager::new(Duration::from_secs(1)));
        let path = journal_path("reconfigure");
        let journal = Arc::new(ConsequentialJournal::open(&path).await.unwrap());
        configure_windows_consequential_control_for_sessions(&sessions, Some(journal.clone()));
        let first = windows_consequential_control_for_sessions(&sessions).unwrap();
        let plan = pending_plan();
        first.pending.lock().await.insert(plan.queued.action.id, plan);
        assert_eq!(first.pending.lock().await.len(), 1);

        configure_windows_consequential_control_for_sessions(&sessions, Some(journal));
        let restarted = windows_consequential_control_for_sessions(&sessions).unwrap();
        assert!(
            restarted.pending.lock().await.is_empty(),
            "durable journal reuse must never reconstruct process-local confirmation authority"
        );

        configure_windows_consequential_control_for_sessions(&sessions, None);
        drop(first);
        drop(restarted);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_postconditions_fail_closed_on_duplicates_or_unknown_version() {
        let v1 = "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"name\":\"Done\"}}".to_owned();
        assert!(validate_postcondition_contracts(std::slice::from_ref(&v1)).is_ok());
        assert!(validate_postcondition_contracts(&[v1.clone(), v1]).is_err());
        assert!(
            validate_postcondition_contracts(&[
                "lvpc:native-semantic:v3:{\"comparison\":\"at_least\",\"count\":1,\"matcher\":{\"name\":\"Done\"}}".into(),
            ])
            .is_err()
        );
    }
}
