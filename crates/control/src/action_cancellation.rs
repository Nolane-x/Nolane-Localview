use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use localview_live_bridge::ActionCancellationState;
use localview_protocol::SessionId;
use serde::Deserialize;
use uuid::Uuid;

use crate::ControlState;

const MAX_CANCELLATION_SIGNALS: usize = 32;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/actions/cancel",
            post(cancel_action),
        )
        .route(
            "/v1/sessions/{id}/actions/cancellations",
            get(action_cancellations),
        )
        .route(
            "/v1/sessions/{id}/actions/cancellations/{action_id}",
            get(action_cancellation),
        )
        .route(
            "/v1/sessions/{id}/actions/cancellations/{action_id}/ack",
            post(acknowledge_action_cancellation),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelActionRequest {
    action_id: Uuid,
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

async fn cancel_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<CancelActionRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    let Some(outcome) = state
        .live
        .request_action_cancellation(id, request.action_id)
        .await
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "action_not_found"})),
        )
            .into_response();
    };

    let status = match outcome.state {
        ActionCancellationState::CancellationRequested => StatusCode::ACCEPTED,
        ActionCancellationState::Cancelled => StatusCode::OK,
    };
    (status, Json(outcome)).into_response()
}

async fn action_cancellations(
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
            .action_cancellations(id, MAX_CANCELLATION_SIGNALS)
            .await,
    )
    .into_response()
}

async fn action_cancellation(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, action_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    match state.live.action_cancellation(id, action_id).await {
        Some(signal) => Json(signal).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn acknowledge_action_cancellation(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, action_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    if state
        .live
        .acknowledge_action_cancellation(id, action_id)
        .await
    {
        return StatusCode::NO_CONTENT.into_response();
    }

    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({"error": "action_cancellation_not_pending"})),
    )
        .into_response()
}
