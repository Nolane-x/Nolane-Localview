use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use chrono::{TimeZone, Utc};
use localview_evidence::{EvidenceDraft, EvidenceKind, UncertaintyClass};
use localview_protocol::SessionId;
use localview_responsive::{
    ContactSheetPolicy, ResponsivePresetId, plan_canonical_sweep, project_contact_sheet,
};
use serde::{Deserialize, Serialize};

use crate::ControlState;

const MAX_REVISION_BYTES: usize = 512;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/evidence/visual-responsive",
            post(ingest_responsive_visual_evidence),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponsiveVisualEvidenceRequest {
    artifact_id: String,
    route: String,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    contact_sheet_pixel_width: u32,
    contact_sheet_pixel_height: u32,
    viewports: Vec<ResponsiveViewportEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponsiveViewportEvidence {
    preset: ResponsivePresetId,
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
    pixel_width: u32,
    pixel_height: u32,
    sheet_x: u32,
    sheet_y: u32,
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

fn valid_native_pixel_dimensions(viewport: &ResponsiveViewportEvidence) -> bool {
    if viewport.css_width == 0
        || viewport.css_height == 0
        || viewport.pixel_width == 0
        || viewport.pixel_height == 0
        || !viewport.device_scale_factor.is_finite()
        || viewport.device_scale_factor <= 0.0
        || viewport.device_scale_factor > 8.0
    {
        return false;
    }

    let expected_width = f64::from(viewport.css_width) * viewport.device_scale_factor;
    let expected_height = f64::from(viewport.css_height) * viewport.device_scale_factor;
    if !expected_width.is_finite() || !expected_height.is_finite() {
        return false;
    }

    (expected_width - f64::from(viewport.pixel_width)).abs() <= 1.0
        && (expected_height - f64::from(viewport.pixel_height)).abs() <= 1.0
}

fn valid_responsive_geometry(request: &ResponsiveVisualEvidenceRequest) -> bool {
    if request.viewports.is_empty() || request.viewports.len() > 4 {
        return false;
    }

    let presets = request
        .viewports
        .iter()
        .map(|viewport| viewport.preset)
        .collect::<Vec<_>>();
    let Ok(plan) = plan_canonical_sweep(&presets) else {
        return false;
    };
    if plan.presets != presets {
        return false;
    }

    for (index, viewport) in request.viewports.iter().enumerate() {
        let canonical = plan.viewports[index];
        if viewport.css_width != canonical.width
            || viewport.css_height != canonical.height
            || !valid_native_pixel_dimensions(viewport)
        {
            return false;
        }
    }

    let pixel_dimensions = request
        .viewports
        .iter()
        .map(|viewport| (viewport.pixel_width, viewport.pixel_height))
        .collect::<Vec<_>>();
    let Ok(projected) =
        project_contact_sheet(&plan, &pixel_dimensions, ContactSheetPolicy::default())
    else {
        return false;
    };

    if projected.pixel_width != request.contact_sheet_pixel_width
        || projected.pixel_height != request.contact_sheet_pixel_height
        || projected.placements.len() != request.viewports.len()
    {
        return false;
    }

    request
        .viewports
        .iter()
        .zip(projected.placements.iter())
        .all(|(provided, expected)| {
            provided.sheet_x == expected.x
                && provided.sheet_y == expected.y
                && provided.pixel_width == expected.pixel_width
                && provided.pixel_height == expected.pixel_height
        })
}

async fn ingest_responsive_visual_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<ResponsiveVisualEvidenceRequest>,
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
            Json(serde_json::json!({"error": "invalid_responsive_visual_evidence"})),
        )
            .into_response();
    };

    if !valid_artifact_id(&request.artifact_id)
        || request.captured_at_unix_ms < 0
        || request
            .revision
            .as_ref()
            .is_some_and(|revision| revision.len() > MAX_REVISION_BYTES)
        || !valid_responsive_geometry(&request)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_responsive_visual_evidence"})),
        )
            .into_response();
    }

    let Some(captured_at) = Utc
        .timestamp_millis_opt(request.captured_at_unix_ms)
        .single()
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_capture_timestamp"})),
        )
            .into_response();
    };

    let payload = serde_json::json!({
        "artifact_id": request.artifact_id,
        "route": route,
        "target": "responsive_contact_sheet",
        "contact_sheet_pixel_width": request.contact_sheet_pixel_width,
        "contact_sheet_pixel_height": request.contact_sheet_pixel_height,
        "viewports": request.viewports,
    });

    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Visual,
            session_id: id,
            region: Some("responsive_contact_sheet".into()),
            payload,
            provenance: localview_evidence::Provenance {
                source: "native-capture".into(),
                engine: Some("responsive-contact-sheet".into()),
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
