use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_evidence::{EvidenceKind, UncertaintyClass};
use localview_protocol::SessionId;
use localview_verification::{
    verify_visual_change, VisualChangeExpectation, VisualChangeObservation,
};
use serde::Deserialize;

use crate::ControlState;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/verify/visual",
            post(verify_retained_visual_diff),
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

fn invalid_verification() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "invalid_visual_verification"})),
    )
        .into_response()
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
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response();
    }

    let Some(evidence) = state.evidence.get(&request.evidence_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "evidence_not_found"})),
        )
            .into_response();
    };

    if evidence.session_id != id
        || evidence.kind != EvidenceKind::Contract
        || evidence.provenance.source != "native-visual-diff"
        || evidence.provenance.engine.as_deref() != Some("pixel-diff")
        || evidence.uncertainty != UncertaintyClass::Observed
        || evidence.secret_taint
        || evidence.confidence < 1.0
    {
        return invalid_verification();
    }

    let Ok(payload) = serde_json::from_value::<RetainedVisualDiffPayload>(evidence.payload) else {
        return invalid_verification();
    };
    if !trusted_diff_shape(&payload) {
        return invalid_verification();
    }

    let observation = VisualChangeObservation {
        changed_ratio: payload.changed_ratio,
        baseline_comparable: payload.baseline_comparable,
    };
    let Ok(result) = verify_visual_change(&observation, request.expectation) else {
        return invalid_verification();
    };

    Json(serde_json::json!({
        "evidence_id": request.evidence_id,
        "result": result,
    }))
    .into_response()
}
