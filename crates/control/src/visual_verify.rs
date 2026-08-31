use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_evidence::{EvidenceKind, UncertaintyClass};
use localview_live_bridge::{NativeExecutorAction, NativeExecutorResult};
use localview_protocol::{SessionId, ViewportMeta};
use localview_resource_governor::ResourceWorkKind;
use localview_verification::{
    verify_visual_change, VisualChangeExpectation, VisualChangeObservation,
};
use serde::Deserialize;

use crate::{
    resource_runtime::{denial_response as resource_denial_response, governor as resource_governor},
    ControlState,
};

const NATIVE_VISUAL_DIFF_TIMEOUT: Duration = Duration::from_secs(12);
const NATIVE_VISUAL_DIFF_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeVisualDiffWaitError {
    Timeout,
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/verify/visual",
            post(verify_retained_visual_diff),
        )
        .route(
            "/v1/sessions/{id}/verify/visual/capture",
            post(capture_and_verify_visual_diff),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveVisualVerifyRequest {
    evidence_id: String,
    expectation: VisualChangeExpectation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveVisualCaptureVerifyRequest {
    viewport: ViewportMeta,
    revision: Option<String>,
    expectation: VisualChangeExpectation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedVisualDiffPayload {
    route: String,
    viewport: RetainedViewport,
    mode: RetainedVisualDiffMode,
    changed_ratio: f64,
    baseline_comparable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedViewport {
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RetainedVisualDiffMode {
    Unchanged,
    Regions,
    Viewport,
    BaselineReset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedVerificationError {
    EvidenceNotFound,
    Invalid,
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

fn invalid_verification() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "invalid_visual_verification"})),
    )
        .into_response()
}

fn retained_verification_error_response(
    error: RetainedVerificationError,
) -> axum::response::Response {
    match error {
        RetainedVerificationError::EvidenceNotFound => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "evidence_not_found"})),
        )
            .into_response(),
        RetainedVerificationError::Invalid => invalid_verification(),
    }
}

fn trusted_diff_shape(payload: &RetainedVisualDiffPayload) -> bool {
    if payload.route.is_empty()
        || payload.viewport.css_width == 0
        || payload.viewport.css_height == 0
        || !payload.viewport.device_scale_factor.is_finite()
        || payload.viewport.device_scale_factor <= 0.0
        || payload.viewport.device_scale_factor > 8.0
        || !payload.changed_ratio.is_finite()
        || !(0.0..=1.0).contains(&payload.changed_ratio)
    {
        return false;
    }

    match payload.mode {
        RetainedVisualDiffMode::BaselineReset => {
            !payload.baseline_comparable && payload.changed_ratio == 1.0
        }
        RetainedVisualDiffMode::Unchanged => {
            payload.baseline_comparable && payload.changed_ratio == 0.0
        }
        RetainedVisualDiffMode::Regions | RetainedVisualDiffMode::Viewport => {
            payload.baseline_comparable && payload.changed_ratio > 0.0
        }
    }
}

fn valid_capture_viewport(viewport: &ViewportMeta) -> bool {
    viewport.css_width > 0
        && viewport.css_height > 0
        && viewport.device_scale_factor.is_finite()
        && viewport.device_scale_factor > 0.0
        && viewport.device_scale_factor <= 8.0
}

async fn verify_retained_visual_diff(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<LiveVisualVerifyRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    let result = match verify_retained_evidence(
        &state,
        id,
        &request.evidence_id,
        request.expectation,
    )
    .await
    {
        Ok(result) => result,
        Err(error) => return retained_verification_error_response(error),
    };

    Json(serde_json::json!({
        "evidence_id": request.evidence_id,
        "result": result,
    }))
    .into_response()
}

async fn capture_and_verify_visual_diff(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<LiveVisualCaptureVerifyRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }
    if !valid_capture_viewport(&request.viewport) {
        return invalid_verification();
    }

    let reservation_id = uuid::Uuid::new_v4();
    let resource_reservation = match resource_governor(&state).reserve(
        id.to_string(),
        reservation_id.to_string(),
        ResourceWorkKind::NativeVisualCapture,
    ) {
        Ok(reservation) => reservation,
        Err(denial) => return resource_denial_response(denial),
    };

    let native_request = state
        .live
        .enqueue_native_executor(
            id,
            NativeExecutorAction::VisualDiffCapture {
                viewport: request.viewport,
                revision: request.revision,
            },
        )
        .await;
    let native_result = match wait_for_native_visual_diff(&state, id, native_request.id).await {
        Ok(result) => result,
        Err(NativeVisualDiffWaitError::Timeout) => {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(serde_json::json!({
                    "error": "native_visual_diff_timeout",
                    "native_request_id": native_request.id,
                })),
            )
                .into_response();
        }
    };
    drop(resource_reservation);

    if !native_result.ok {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": "native_visual_diff_failed",
                "native_request_id": native_request.id,
            })),
        )
            .into_response();
    }
    let Some(evidence_id) = native_result
        .payload
        .get("visual_diff_evidence_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        return invalid_native_visual_diff_result(native_request.id);
    };

    let result = match verify_retained_evidence(&state, id, &evidence_id, request.expectation).await {
        Ok(result) => result,
        Err(error) => return retained_verification_error_response(error),
    };

    Json(serde_json::json!({
        "evidence_id": evidence_id,
        "result": result,
        "native_request_id": native_request.id,
    }))
    .into_response()
}

async fn wait_for_native_visual_diff(
    state: &ControlState,
    id: SessionId,
    request_id: uuid::Uuid,
) -> Result<NativeExecutorResult, NativeVisualDiffWaitError> {
    wait_for_native_visual_diff_with_timeout(state, id, request_id, NATIVE_VISUAL_DIFF_TIMEOUT).await
}

#[doc(hidden)]
pub async fn wait_for_native_visual_diff_with_timeout(
    state: &ControlState,
    id: SessionId,
    request_id: uuid::Uuid,
    timeout: Duration,
) -> Result<NativeExecutorResult, NativeVisualDiffWaitError> {
    let result = tokio::time::timeout(timeout, async {
        loop {
            if let Some(result) = state
                .live
                .recent_native_executor_results(id, 16)
                .await
                .into_iter()
                .find(|result| result.request_id == request_id)
            {
                return result;
            }
            tokio::time::sleep(NATIVE_VISUAL_DIFF_POLL_INTERVAL).await;
        }
    })
    .await;

    match result {
        Ok(result) => Ok(result),
        Err(_) => {
            let _ = state
                .live
                .request_native_executor_cancellation(id, request_id)
                .await;
            Err(NativeVisualDiffWaitError::Timeout)
        }
    }
}

async fn verify_retained_evidence(
    state: &ControlState,
    id: SessionId,
    evidence_id: &str,
    expectation: VisualChangeExpectation,
) -> Result<serde_json::Value, RetainedVerificationError> {
    let Some(evidence) = state.evidence.get(evidence_id).await else {
        return Err(RetainedVerificationError::EvidenceNotFound);
    };

    if evidence.session_id != id
        || evidence.kind != EvidenceKind::Contract
        || evidence.provenance.source != "native-visual-diff"
        || evidence.provenance.engine.as_deref() != Some("pixel-diff")
        || evidence.uncertainty != UncertaintyClass::Observed
        || evidence.secret_taint
        || evidence.confidence < 1.0
    {
        return Err(RetainedVerificationError::Invalid);
    }

    let Ok(payload) = serde_json::from_value::<RetainedVisualDiffPayload>(evidence.payload) else {
        return Err(RetainedVerificationError::Invalid);
    };
    if !trusted_diff_shape(&payload) {
        return Err(RetainedVerificationError::Invalid);
    }

    let observation = VisualChangeObservation {
        changed_ratio: payload.changed_ratio,
        baseline_comparable: payload.baseline_comparable,
    };
    let Ok(result) = verify_visual_change(&observation, expectation) else {
        return Err(RetainedVerificationError::Invalid);
    };
    serde_json::to_value(result).map_err(|_| RetainedVerificationError::Invalid)
}

fn invalid_native_visual_diff_result(request_id: uuid::Uuid) -> axum::response::Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": "invalid_native_visual_diff_result",
            "native_request_id": request_id,
        })),
    )
        .into_response()
}
