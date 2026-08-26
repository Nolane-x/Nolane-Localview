#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use localview_live_bridge::NativeExecutorResult;
use localview_protocol::SessionId;

use crate::{
    perception::{authorized, denied},
    ControlState,
};

const MAX_NATIVE_EXECUTOR_POLL_BATCH: usize = 8;
const NATIVE_EXECUTOR_ACTIVE_LEASE_SECS: i64 = 15;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/native-executor",
            get(take_native_executor_requests),
        )
        .route(
            "/v1/sessions/{id}/native-executor/results",
            post(complete_native_executor_result),
        )
        .with_state(state)
}

async fn take_native_executor_requests(
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

    state
        .live
        .expire_native_executor_active_before(
            id,
            Utc::now() - chrono::Duration::seconds(NATIVE_EXECUTOR_ACTIVE_LEASE_SECS),
        )
        .await;

    Json(
        state
            .live
            .take_native_executor_requests(id, MAX_NATIVE_EXECUTOR_POLL_BATCH)
            .await,
    )
    .into_response()
}

async fn complete_native_executor_result(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(result): Json<NativeExecutorResult>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    if state
        .live
        .claim_native_executor(id, result.request_id)
        .await
        .is_none()
    {
        return result_without_origin();
    }

    if !state.live.complete_native_executor(id, result).await {
        return result_without_origin();
    }

    StatusCode::NO_CONTENT.into_response()
}

fn session_not_found() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "session_not_found"})),
    )
        .into_response()
}

fn result_without_origin() -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "native_executor_result_without_inflight_origin"
        })),
    )
        .into_response()
}
