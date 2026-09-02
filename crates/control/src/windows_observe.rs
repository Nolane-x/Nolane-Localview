#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use localview_native_provider::UserSelectedWindowTarget;
use localview_protocol::SessionId;
use localview_sessions::SessionManager;
use localview_windows_observe_runtime::{
    WindowsObserveRuntimeError, WindowsUiaObserveRuntimeManager,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::ControlState;

#[derive(Debug)]
struct RuntimeEntry {
    owner: Weak<SessionManager>,
    runtime: Arc<WindowsUiaObserveRuntimeManager>,
}

type RuntimeRegistry = HashMap<usize, RuntimeEntry>;

static RUNTIMES: OnceLock<Mutex<RuntimeRegistry>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowsObserveAttachRequest {
    native_window_handle: u64,
    expected_process_id: u32,
    selection_nonce: Uuid,
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/windows-observe/attach",
            post(attach_windows_observe),
        )
        .route(
            "/v1/sessions/{id}/windows-observe/status",
            get(windows_observe_status),
        )
        .route(
            "/v1/sessions/{id}/windows-observe/detach",
            post(detach_windows_observe),
        )
        .with_state(state)
}

pub fn configure_windows_observe_runtime_for_sessions(
    sessions: &Arc<SessionManager>,
    runtime: Option<Arc<WindowsUiaObserveRuntimeManager>>,
) {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    match runtime {
        Some(runtime) => {
            entries.insert(
                key,
                RuntimeEntry {
                    owner: Arc::downgrade(sessions),
                    runtime,
                },
            );
        }
        None => {
            entries.remove(&key);
        }
    }
}

pub fn windows_observe_runtime_for_sessions(
    sessions: &Arc<SessionManager>,
) -> Option<Arc<WindowsUiaObserveRuntimeManager>> {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get(&key).map(|entry| entry.runtime.clone())
}

async fn attach_windows_observe(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<WindowsObserveAttachRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }
    let Some(runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable();
    };

    let selection = UserSelectedWindowTarget {
        native_window_handle: request.native_window_handle,
        expected_process_id: request.expected_process_id,
        selection_nonce: request.selection_nonce,
    };
    match runtime.attach(id, selection).await {
        Ok(status) => Json(status).into_response(),
        Err(error) => runtime_error_response(error),
    }
}

async fn windows_observe_status(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }
    let Some(runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable();
    };

    match runtime.status(id).await {
        Some(status) => Json(status).into_response(),
        None => not_attached(),
    }
}

async fn detach_windows_observe(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }
    let Some(runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable();
    };

    match runtime.release(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => runtime_error_response(error),
    }
}

fn runtime_error_response(error: WindowsObserveRuntimeError) -> axum::response::Response {
    let status = match error {
        WindowsObserveRuntimeError::AlreadyAttached { .. } => StatusCode::CONFLICT,
        WindowsObserveRuntimeError::NotAttached { .. } => StatusCode::NOT_FOUND,
        WindowsObserveRuntimeError::InvalidConfiguration => StatusCode::INTERNAL_SERVER_ERROR,
        WindowsObserveRuntimeError::Provider { .. }
        | WindowsObserveRuntimeError::ProviderTask { .. } => StatusCode::SERVICE_UNAVAILABLE,
        WindowsObserveRuntimeError::SubscriptionProviderIncarnationMismatch
        | WindowsObserveRuntimeError::SubscriptionTargetIncarnationMismatch
        | WindowsObserveRuntimeError::Bridge(_)
        | WindowsObserveRuntimeError::ObservationStateMissing { .. } => StatusCode::CONFLICT,
    };
    (
        status,
        Json(serde_json::json!({
            "error": "windows_observe_runtime_error",
            "message": error.to_string(),
        })),
    )
        .into_response()
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

fn not_attached() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "windows_observe_not_attached"})),
    )
        .into_response()
}

fn unavailable() -> axum::response::Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "windows_observe_unavailable",
            "message": "Windows observe runtime is unavailable on this daemon"
        })),
    )
        .into_response()
}

fn lock_registry(registry: &Mutex<RuntimeRegistry>) -> MutexGuard<'_, RuntimeRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
