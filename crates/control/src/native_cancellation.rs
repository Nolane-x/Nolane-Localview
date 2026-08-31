use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use localview_live_bridge::NativeExecutorCancellationState;
use localview_protocol::SessionId;
use serde::Deserialize;
use uuid::Uuid;

use crate::ControlState;

const MAX_CANCELLATION_SIGNALS: usize = 32;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/native-executor/cancel",
            post(cancel_native_executor),
        )
        .route(
            "/v1/sessions/{id}/native-executor/cancellations",
            get(native_executor_cancellations),
        )
        .route(
            "/v1/sessions/{id}/native-executor/cancellations/{request_id}/ack",
            post(acknowledge_native_executor_cancellation),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelNativeExecutorRequest {
    request_id: Uuid,
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token.as_ref())
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

async fn cancel_native_executor(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<CancelNativeExecutorRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    let Some(outcome) = state
        .live
        .request_native_executor_cancellation(id, request.request_id)
        .await
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "native_executor_request_not_found"})),
        )
            .into_response();
    };

    let status = match outcome.state {
        NativeExecutorCancellationState::CancellationRequested => StatusCode::ACCEPTED,
        NativeExecutorCancellationState::Cancelled => StatusCode::OK,
    };
    (status, Json(outcome)).into_response()
}

async fn native_executor_cancellations(
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

    Json(
        state
            .live
            .native_executor_cancellations(id, MAX_CANCELLATION_SIGNALS)
            .await,
    )
    .into_response()
}

async fn acknowledge_native_executor_cancellation(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, request_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    if state
        .live
        .acknowledge_native_executor_cancellation(id, request_id)
        .await
    {
        return StatusCode::NO_CONTENT.into_response();
    }

    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": "native_executor_cancellation_not_pending"})),
    )
        .into_response()
}
