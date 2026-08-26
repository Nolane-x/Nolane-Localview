#![forbid(unsafe_code)]

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_resource_governor::{ResourceAdmissionDenial, RuntimeResourceSample};

use crate::{
    perception::{authorized, denied},
    ControlState,
};

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route("/v1/runtime/resources/sample", post(update_runtime_sample))
        .with_state(state)
}

async fn update_runtime_sample(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(sample): Json<RuntimeResourceSample>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !state.resources.update_sample(sample) {
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
