use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use chrono::{TimeZone, Utc};
use localview_evidence::{EvidenceDraft, EvidenceKind, UncertaintyClass};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};

use crate::ControlState;

const MAX_FULL_PAGE_TILES: usize = 32;
const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;
const MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT: u32 = 32_768;
const MAX_FULL_PAGE_OUTPUT_RGBA_BYTES: u64 = 128 * 1024 * 1024;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/evidence/visual-full-page",
            post(ingest_full_page_visual_evidence),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FullPageVisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: FullPageVisualViewport,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    document_css_width: f64,
    document_css_height: f64,
    tile_count: usize,
    scroll_offsets_y: Vec<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FullPageVisualViewport {
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

fn canonical_loopback_route(value: &str) -> Option<String> {
    let mut route = url::Url::parse(value).ok()?;
    if !matches!(route.scheme(), "http" | "https") {
        return None;
    }
    let loopback = route.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !loopback {
        return None;
    }
    route.set_query(None);
    route.set_fragment(None);
    Some(route.to_string())
}

fn valid_output_bounds(request: &FullPageVisualEvidenceRequest) -> bool {
    if request.pixel_width == 0
        || request.pixel_height == 0
        || request.pixel_height > MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT
    {
        return false;
    }
    u64::from(request.pixel_width)
        .checked_mul(u64::from(request.pixel_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .is_some_and(|bytes| bytes <= MAX_FULL_PAGE_OUTPUT_RGBA_BYTES)
}

fn expected_scroll_offsets(
    document_css_height: f64,
    viewport_css_height: f64,
) -> Option<Vec<f64>> {
    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    let mut offsets = vec![0.0];
    if max_scroll_y > 0.0 {
        let mut next = viewport_css_height;
        while next < max_scroll_y {
            if offsets.len() >= MAX_FULL_PAGE_TILES {
                return None;
            }
            offsets.push(next);
            next += viewport_css_height;
            if !next.is_finite() {
                return None;
            }
        }
        if offsets.last().copied() != Some(max_scroll_y) {
            if offsets.len() >= MAX_FULL_PAGE_TILES {
                return None;
            }
            offsets.push(max_scroll_y);
        }
    }
    Some(offsets)
}

fn valid_geometry_and_plan(request: &FullPageVisualEvidenceRequest) -> bool {
    if request.viewport.css_width == 0
        || request.viewport.css_height == 0
        || !request.viewport.device_scale_factor.is_finite()
        || request.viewport.device_scale_factor <= 0.0
        || !request.document_css_width.is_finite()
        || !request.document_css_height.is_finite()
        || request.document_css_width <= 0.0
        || request.document_css_height <= 0.0
        || request.document_css_height > MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT
        || request.document_css_width != f64::from(request.viewport.css_width)
        || request.tile_count == 0
        || request.tile_count > MAX_FULL_PAGE_TILES
        || request.scroll_offsets_y.len() != request.tile_count
        || request
            .scroll_offsets_y
            .iter()
            .any(|offset| !offset.is_finite() || *offset < 0.0)
    {
        return false;
    }

    expected_scroll_offsets(
        request.document_css_height,
        f64::from(request.viewport.css_height),
    )
    .is_some_and(|expected| expected == request.scroll_offsets_y)
}

async fn ingest_full_page_visual_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<FullPageVisualEvidenceRequest>,
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

    let Some(route) = canonical_loopback_route(&request.route) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_full_page_visual_evidence"})),
        )
            .into_response();
    };

    if !valid_artifact_id(&request.artifact_id)
        || !valid_backend(&request.backend)
        || request.captured_at_unix_ms < 0
        || !valid_output_bounds(&request)
        || !valid_geometry_and_plan(&request)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_full_page_visual_evidence"})),
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
        "route": route,
        "viewport": request.viewport,
        "target": "full_page",
        "document_css_width": request.document_css_width,
        "document_css_height": request.document_css_height,
        "tile_count": request.tile_count,
        "scroll_offsets_y": request.scroll_offsets_y,
    });
    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Visual,
            session_id: id,
            region: Some("full_page".into()),
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
