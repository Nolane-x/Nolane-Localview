#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_resource_governor::{
    ResourceAdmissionDenial, RuntimeResourceGovernor, RuntimeResourceSample,
};
use localview_sessions::SessionManager;

use crate::{
    perception::{authorized, denied},
    ControlState,
};

#[derive(Debug)]
struct GovernorEntry {
    owner: Weak<SessionManager>,
    governor: RuntimeResourceGovernor,
}

type GovernorRegistry = HashMap<usize, GovernorEntry>;

static GOVERNORS: OnceLock<Mutex<GovernorRegistry>> = OnceLock::new();

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route("/v1/runtime/resources/sample", post(update_runtime_sample))
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
