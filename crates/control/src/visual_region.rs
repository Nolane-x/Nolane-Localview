use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use chrono::{TimeZone, Utc};
use localview_evidence::{EvidenceDraft, EvidenceKind, UncertaintyClass};
use localview_protocol::{Rect, SessionId};
use serde::{Deserialize, Serialize};

use crate::ControlState;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/evidence/visual-region",
            post(ingest_region_visual_evidence),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionVisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: VisualViewport,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    target: String,
    region: Rect,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualViewport {
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token.as_ref())
}

fn valid_artifact_id(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("lv-") else {
        return false;
    };
    digest.len() == 16
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_backend(value: &str) -> bool {
    matches!(value, "webview2" | "wk_web_view" | "web_kit_gtk")
}

fn valid_route(value: &str) -> bool {
    let Ok(route) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(route.scheme(), "http" | "https") {
        return false;
    }
    route.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn valid_region(viewport: &VisualViewport, region: &Rect) -> bool {
    if viewport.css_width == 0
        || viewport.css_height == 0
        || !viewport.device_scale_factor.is_finite()
        || viewport.device_scale_factor <= 0.0
    {
        return false;
    }

    let right = region.x + region.width;
    let bottom = region.y + region.height;
    region.x.is_finite()
        && region.y.is_finite()
        && region.width.is_finite()
        && region.height.is_finite()
        && right.is_finite()
        && bottom.is_finite()
        && region.x >= 0.0
        && region.y >= 0.0
        && region.width > 0.0
        && region.height > 0.0
        && right <= viewport.css_width as f64
        && bottom <= viewport.css_height as f64
}

async fn ingest_region_visual_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<RegionVisualEvidenceRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    if state.sessions.get(id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response();
    }

    if request.target != "region"
        || !valid_artifact_id(&request.artifact_id)
        || !valid_backend(&request.backend)
        || request.pixel_width == 0
        || request.pixel_height == 0
        || !valid_route(&request.route)
        || request.captured_at_unix_ms < 0
        || !valid_region(&request.viewport, &request.region)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_region_visual_evidence"})),
        )
            .into_response();
    }

    let Some(captured_at) = Utc.timestamp_millis_opt(request.captured_at_unix_ms).single() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_capture_timestamp"})),
        )
            .into_response();
    };

    let backend = request.backend.clone();
    let payload = serde_json::json!({
        "artifact_id": request.artifact_id,
        "pixel_width": request.pixel_width,
        "pixel_height": request.pixel_height,
        "backend": request.backend,
        "route": request.route,
        "viewport": request.viewport,
        "target": "region",
        "region": request.region,
    });
    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Visual,
            session_id: id,
            region: Some("region".into()),
            payload,
            provenance: localview_evidence::Provenance {
                source: "native-capture".into(),
                engine: Some(backend),
                revision: request.revision,
                parent_ids: Vec::new(),
                captured_at,
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        })
        .await;

    Json(serde_json::json!({
        "evidence_id": stored.id,
        "deduplicated": stored.deduplicated,
    }))
    .into_response()
}
