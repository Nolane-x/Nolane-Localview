#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_protocol::SessionId;
use localview_resource_governor::{
    LiveResourceLease, LiveSurfaceIdentity, ResourceAdmissionDenial, ResourceReservation,
    ResourceWorkKind, RuntimeResourceGovernor, RuntimeResourceSample, SurfaceVisibility,
};
use localview_sessions::SessionManager;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    perception::{authorized, denied},
    surface_liveness::reap_expired_surface_owner_resources_for_sessions,
    surface_owner::{
        register_surface_owner_for_sessions, validate_surface_owner_for_sessions,
        SurfaceOwnerError, SurfaceOwnerProof,
    },
    surface_recovery::{surface_recovery_journal_for_sessions, SurfaceRecoveryKey},
    ControlState,
};

#[derive(Debug)]
struct GovernorEntry {
    owner: Weak<SessionManager>,
    governor: RuntimeResourceGovernor,
}

type GovernorRegistry = HashMap<usize, GovernorEntry>;

static GOVERNORS: OnceLock<Mutex<GovernorRegistry>> = OnceLock::new();

#[derive(Debug)]
struct SurfaceResourceEntry {
    owner: Weak<SessionManager>,
    pending: BTreeMap<(Uuid, SessionId, String), ResourceReservation>,
    activating: BTreeSet<(Uuid, SessionId, LiveSurfaceIdentity)>,
    live: BTreeMap<(Uuid, SessionId, LiveSurfaceIdentity), LiveResourceLease>,
}

type SurfaceResourceRegistry = HashMap<usize, SurfaceResourceEntry>;

static SURFACE_RESOURCES: OnceLock<Mutex<SurfaceResourceRegistry>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceOwnerRegisterRequest {
    owner_instance_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceReserveRequest {
    session_id: SessionId,
    request_id: String,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceReserveRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceActivateRequest {
    session_id: SessionId,
    request_id: String,
    surface_kind: String,
    label: String,
    incarnation: u64,
    visibility: SurfaceVisibility,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceActivateRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceReattachRequest {
    session_id: SessionId,
    surface_kind: String,
    label: String,
    incarnation: u64,
    visibility: SurfaceVisibility,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceReattachRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceVisibilityRequest {
    session_id: SessionId,
    surface_kind: String,
    label: String,
    incarnation: u64,
    visibility: SurfaceVisibility,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceVisibilityRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceReleaseRequest {
    session_id: SessionId,
    surface_kind: String,
    label: String,
    incarnation: u64,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceReleaseRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route("/v1/runtime/resources/sample", post(update_runtime_sample))
        .route(
            "/v1/runtime/resources/surfaces/owners/register",
            post(register_surface_owner),
        )
        .route(
            "/v1/runtime/resources/surfaces/reserve",
            post(reserve_surface_resource),
        )
        .route(
            "/v1/runtime/resources/surfaces/cancel",
            post(cancel_surface_reservation),
        )
        .route(
            "/v1/runtime/resources/surfaces/activate",
            post(activate_surface_resource),
        )
        .route(
            "/v1/runtime/resources/surfaces/reattach",
            post(reattach_surface_resource),
        )
        .route(
            "/v1/runtime/resources/surfaces/visibility",
            post(update_surface_visibility),
        )
        .route(
            "/v1/runtime/resources/surfaces/release",
            post(release_surface_resource),
        )
        .with_state(state)
}

pub fn runtime_resource_governor_for_sessions(
    sessions: &Arc<SessionManager>,
) -> RuntimeResourceGovernor {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = GOVERNORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries
        .entry(key)
        .or_insert_with(|| GovernorEntry {
            owner: Arc::downgrade(sessions),
            governor: RuntimeResourceGovernor::default(),
        })
        .governor
        .clone()
}

pub fn release_surface_resource_session_for_sessions(
    sessions: &Arc<SessionManager>,
    session_id: SessionId,
) -> usize {
    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let Some(entry) = existing_surface_entry_mut(&mut entries, sessions) else {
        return 0;
    };
    let before = entry.pending.len();
    entry
        .pending
        .retain(|(_, pending_session, _), _| *pending_session != session_id);
    before.saturating_sub(entry.pending.len())
}

pub(crate) fn release_surface_resource_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    owner_instance_id: Uuid,
) -> usize {
    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let Some(entry) = existing_surface_entry_mut(&mut entries, sessions) else {
        return 0;
    };

    let pending_before = entry.pending.len();
    entry
        .pending
        .retain(|(owner, _, _), _| *owner != owner_instance_id);
    let pending_removed = pending_before.saturating_sub(entry.pending.len());

    entry
        .activating
        .retain(|(owner, _, _)| *owner != owner_instance_id);

    let live_before = entry.live.len();
    entry
        .live
        .retain(|(owner, _, _), _| *owner != owner_instance_id);
    let live_removed = live_before.saturating_sub(entry.live.len());

    pending_removed.saturating_add(live_removed)
}

pub(crate) fn governor(state: &ControlState) -> RuntimeResourceGovernor {
    runtime_resource_governor_for_sessions(&state.sessions)
}

async fn update_runtime_sample(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(sample): Json<RuntimeResourceSample>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !governor(&state).update_sample(sample) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_runtime_resource_sample"})),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn register_surface_owner(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceOwnerRegisterRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if request.owner_instance_id.is_nil() {
        return surface_bad_request("invalid_surface_owner_instance");
    }
    let _ = reap_expired_surface_owner_resources_for_sessions(&state.sessions);
    Json(register_surface_owner_for_sessions(
        &state.sessions,
        request.owner_instance_id,
    ))
    .into_response()
}

async fn reserve_surface_resource(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceReserveRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    if state.sessions.get(request.session_id).await.is_none() {
        return surface_not_found("surface_session_not_found");
    }
    if !valid_request_id(&request.request_id) {
        return surface_bad_request("invalid_surface_request_id");
    }

    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let entry = surface_entry_mut(&mut entries, &state.sessions);
    let key = (
        proof.owner_instance_id,
        request.session_id,
        request.request_id.clone(),
    );
    if entry.pending.contains_key(&key) {
        return surface_conflict("surface_reservation_already_exists");
    }

    let reservation = match governor(&state).reserve(
        request.session_id.to_string(),
        request.request_id,
        ResourceWorkKind::NativeSurface,
    ) {
        Ok(reservation) => reservation,
        Err(denial) => return denial_response(denial),
    };
    entry.pending.insert(key, reservation);
    StatusCode::NO_CONTENT.into_response()
}

async fn cancel_surface_reservation(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceReserveRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    if state.sessions.get(request.session_id).await.is_none() {
        return surface_not_found("surface_session_not_found");
    }
    if !valid_request_id(&request.request_id) {
        return surface_bad_request("invalid_surface_request_id");
    }

    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let Some(entry) = existing_surface_entry_mut(&mut entries, &state.sessions) else {
        return surface_conflict("surface_reservation_missing");
    };
    if entry
        .pending
        .remove(&(
            proof.owner_instance_id,
            request.session_id,
            request.request_id,
        ))
        .is_none()
    {
        return surface_conflict("surface_reservation_missing");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn activate_surface_resource(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceActivateRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    if request.incarnation == 0 {
        return surface_bad_request("invalid_surface_identity");
    }
    let Some(identity) = surface_identity(
        request.surface_kind,
        request.label,
        request.incarnation,
    ) else {
        return surface_bad_request("invalid_surface_identity");
    };
    let Some(recovery) = surface_recovery_journal_for_sessions(&state.sessions) else {
        return surface_recovery_unavailable();
    };
    let recovery_key = match SurfaceRecoveryKey::new(
        request.session_id,
        identity.surface_kind.clone(),
        identity.label.clone(),
        identity.incarnation,
        proof.owner_instance_id,
    ) {
        Ok(key) => key,
        Err(_) => return surface_bad_request("invalid_surface_identity"),
    };
    let claim = (proof.owner_instance_id, request.session_id, identity.clone());

    let reservation = {
        let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut entries = lock_surface_registry(registry);
        let entry = surface_entry_mut(&mut entries, &state.sessions);
        if surface_logically_claimed(entry, request.session_id, &identity) {
            return surface_conflict("surface_owner_already_live");
        }

        let pending_key = (
            proof.owner_instance_id,
            request.session_id,
            request.request_id,
        );
        let Some(reservation) = entry.pending.remove(&pending_key) else {
            return surface_conflict("surface_reservation_missing");
        };
        entry.activating.insert(claim.clone());
        reservation
    };

    let lease = match reservation.activate_surface(identity.clone(), request.visibility) {
        Ok(lease) => lease,
        Err(_) => {
            clear_activating_claim(&state.sessions, &claim);
            return surface_conflict("surface_activation_rejected");
        }
    };

    if recovery.record_activated(recovery_key).await.is_err() {
        clear_activating_claim(&state.sessions, &claim);
        drop(lease);
        return surface_recovery_unavailable();
    }

    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let entry = surface_entry_mut(&mut entries, &state.sessions);
    entry.activating.remove(&claim);
    entry.live.insert(claim, lease);
    StatusCode::NO_CONTENT.into_response()
}

async fn reattach_surface_resource(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceReattachRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    if state.sessions.get(request.session_id).await.is_none() {
        return surface_not_found("surface_session_not_found");
    }
    if request.incarnation == 0 {
        return surface_bad_request("invalid_surface_identity");
    }
    let Some(identity) = surface_identity(
        request.surface_kind,
        request.label,
        request.incarnation,
    ) else {
        return surface_bad_request("invalid_surface_identity");
    };
    let Some(recovery) = surface_recovery_journal_for_sessions(&state.sessions) else {
        return surface_recovery_unavailable();
    };
    let recovery_key = match SurfaceRecoveryKey::new(
        request.session_id,
        identity.surface_kind.clone(),
        identity.label.clone(),
        identity.incarnation,
        proof.owner_instance_id,
    ) {
        Ok(key) => key,
        Err(_) => return surface_bad_request("invalid_surface_identity"),
    };
    if !recovery.outstanding_exact(&recovery_key) {
        return surface_conflict("surface_recovery_debt_missing");
    }

    let claim = (proof.owner_instance_id, request.session_id, identity.clone());
    let reservation = {
        let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut entries = lock_surface_registry(registry);
        let entry = surface_entry_mut(&mut entries, &state.sessions);
        if surface_logically_claimed(entry, request.session_id, &identity) {
            return surface_conflict("surface_owner_already_live");
        }

        let reservation = match governor(&state).reserve(
            request.session_id.to_string(),
            format!("surface-reattach-{}", Uuid::new_v4()),
            ResourceWorkKind::NativeSurface,
        ) {
            Ok(reservation) => reservation,
            Err(denial) => return denial_response(denial),
        };
        entry.activating.insert(claim.clone());
        reservation
    };

    let lease = match reservation.activate_surface(identity.clone(), request.visibility) {
        Ok(lease) => lease,
        Err(_) => {
            clear_activating_claim(&state.sessions, &claim);
            return surface_conflict("surface_activation_rejected");
        }
    };

    if recovery.record_reattached(recovery_key).await.is_err() {
        clear_activating_claim(&state.sessions, &claim);
        drop(lease);
        return surface_recovery_unavailable();
    }

    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let entry = surface_entry_mut(&mut entries, &state.sessions);
    entry.activating.remove(&claim);
    entry.live.insert(claim, lease);
    StatusCode::NO_CONTENT.into_response()
}

async fn update_surface_visibility(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceVisibilityRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    let Some(identity) = surface_identity(
        request.surface_kind,
        request.label,
        request.incarnation,
    ) else {
        return surface_bad_request("invalid_surface_identity");
    };

    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    let Some(entry) = existing_surface_entry_mut(&mut entries, &state.sessions) else {
        return surface_conflict("surface_owner_missing");
    };
    let key = (proof.owner_instance_id, request.session_id, identity.clone());
    let Some(lease) = entry.live.get(&key) else {
        return if surface_owned_by_other_owner(
            entry,
            proof.owner_instance_id,
            request.session_id,
            &identity,
        ) {
            surface_conflict("surface_owner_fence_mismatch")
        } else {
            surface_conflict("surface_owner_incarnation_mismatch")
        };
    };
    if lease
        .set_surface_visibility(identity, request.visibility)
        .is_err()
    {
        return surface_conflict("surface_visibility_rejected");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn release_surface_resource(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceReleaseRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let proof = request.owner_proof();
    if let Err(error) = validate_surface_owner_for_sessions(&state.sessions, proof) {
        return surface_owner_conflict(error);
    }
    let Some(identity) = surface_identity(
        request.surface_kind,
        request.label,
        request.incarnation,
    ) else {
        return surface_bad_request("invalid_surface_identity");
    };
    let recovery = surface_recovery_journal_for_sessions(&state.sessions);

    let lease = {
        let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut entries = lock_surface_registry(registry);
        let Some(entry) = existing_surface_entry_mut(&mut entries, &state.sessions) else {
            return surface_conflict("surface_owner_missing");
        };
        let key = (proof.owner_instance_id, request.session_id, identity.clone());
        if !entry.live.contains_key(&key) {
            return if surface_owned_by_other_owner(
                entry,
                proof.owner_instance_id,
                request.session_id,
                &identity,
            ) {
                surface_conflict("surface_owner_fence_mismatch")
            } else {
                surface_conflict("surface_owner_incarnation_mismatch")
            };
        }
        entry.live.remove(&key).expect("checked exact live surface")
    };
    drop(lease);

    let recovery_key = match SurfaceRecoveryKey::new(
        request.session_id,
        identity.surface_kind.clone(),
        identity.label.clone(),
        identity.incarnation,
        proof.owner_instance_id,
    ) {
        Ok(key) => key,
        Err(_) => return surface_bad_request("invalid_surface_identity"),
    };
    let Some(recovery) = recovery else {
        return surface_recovery_unavailable();
    };
    if recovery.record_released(recovery_key).await.is_err() {
        return surface_recovery_unavailable();
    }
    StatusCode::NO_CONTENT.into_response()
}

fn surface_logically_claimed(
    entry: &SurfaceResourceEntry,
    session_id: SessionId,
    identity: &LiveSurfaceIdentity,
) -> bool {
    entry
        .live
        .keys()
        .chain(entry.activating.iter())
        .any(|(_, current_session, current)| {
            *current_session == session_id
                && current.surface_kind == identity.surface_kind
                && current.label == identity.label
        })
}

fn surface_owned_by_other_owner(
    entry: &SurfaceResourceEntry,
    owner_instance_id: Uuid,
    session_id: SessionId,
    identity: &LiveSurfaceIdentity,
) -> bool {
    entry.live.keys().any(|(current_owner, current_session, current)| {
        *current_owner != owner_instance_id
            && *current_session == session_id
            && current.surface_kind == identity.surface_kind
            && current.label == identity.label
    })
}

fn clear_activating_claim(
    sessions: &Arc<SessionManager>,
    claim: &(Uuid, SessionId, LiveSurfaceIdentity),
) {
    let registry = SURFACE_RESOURCES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_surface_registry(registry);
    if let Some(entry) = existing_surface_entry_mut(&mut entries, sessions) {
        entry.activating.remove(claim);
    }
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 160 && !value.chars().any(char::is_control)
}

fn surface_identity(
    surface_kind: String,
    label: String,
    incarnation: u64,
) -> Option<LiveSurfaceIdentity> {
    if !matches!(surface_kind.as_str(), "preview_window" | "workspace_child")
        || label.is_empty()
        || label.len() > 160
        || label.chars().any(char::is_control)
    {
        return None;
    }
    Some(LiveSurfaceIdentity::new(surface_kind, label, incarnation))
}

fn surface_entry_mut<'a>(
    entries: &'a mut SurfaceResourceRegistry,
    sessions: &Arc<SessionManager>,
) -> &'a mut SurfaceResourceEntry {
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    let key = Arc::as_ptr(sessions) as usize;
    entries.entry(key).or_insert_with(|| SurfaceResourceEntry {
        owner: Arc::downgrade(sessions),
        pending: BTreeMap::new(),
        activating: BTreeSet::new(),
        live: BTreeMap::new(),
    })
}

fn existing_surface_entry_mut<'a>(
    entries: &'a mut SurfaceResourceRegistry,
    sessions: &Arc<SessionManager>,
) -> Option<&'a mut SurfaceResourceEntry> {
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get_mut(&(Arc::as_ptr(sessions) as usize))
}

fn surface_bad_request(error: &'static str) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": error})),
    )
        .into_response()
}

fn surface_conflict(error: &'static str) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": error})),
    )
        .into_response()
}

fn surface_recovery_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "surface_recovery_journal_unavailable"
        })),
    )
        .into_response()
}

fn surface_owner_conflict(error: SurfaceOwnerError) -> axum::response::Response {
    surface_conflict(match error {
        SurfaceOwnerError::NotRegistered => "surface_owner_not_registered",
        SurfaceOwnerError::BootEpochMismatch => "surface_owner_boot_epoch_mismatch",
        SurfaceOwnerError::LeaseMismatch => "surface_owner_lease_mismatch",
    })
}

fn surface_not_found(error: &'static str) -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": error})),
    )
        .into_response()
}

pub(crate) fn denial_response(denial: ResourceAdmissionDenial) -> axum::response::Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "resource_governor_denied",
            "work_kind": denial.work_kind,
            "pressure": denial.decision.pressure,
            "actions": denial.decision.actions,
            "reasons": denial.decision.reasons,
        })),
    )
        .into_response()
}

fn lock_registry(registry: &Mutex<GovernorRegistry>) -> MutexGuard<'_, GovernorRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_surface_registry(
    registry: &Mutex<SurfaceResourceRegistry>,
) -> MutexGuard<'_, SurfaceResourceRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
