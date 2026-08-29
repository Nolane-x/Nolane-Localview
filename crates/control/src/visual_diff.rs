use std::collections::HashSet;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use chrono::{TimeZone, Utc};
use localview_evidence::{EvidenceDraft, EvidenceKind, Provenance, UncertaintyClass};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};

use crate::ControlState;

const MAX_VISUAL_DIFF_PARENTS: usize = 256;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/evidence/visual-diff",
            post(ingest_visual_diff_evidence),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualDiffEvidenceRequest {
    route: String,
    viewport: VisualViewport,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    mode: VisualDiffMode,
    changed_ratio: f64,
    visual_evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct VisualViewport {
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum VisualDiffMode {
    Unchanged,
    Regions,
    Viewport,
    BaselineReset,
}

impl VisualDiffMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Regions => "regions",
            Self::Viewport => "viewport",
            Self::BaselineReset => "baseline_reset",
        }
    }

    fn baseline_comparable(self) -> bool {
        self != Self::BaselineReset
    }

    fn expected_parent_target(self) -> Option<&'static str> {
        match self {
            Self::Unchanged => None,
            Self::Regions => Some("region"),
            Self::Viewport | Self::BaselineReset => Some("viewport"),
        }
    }
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

fn bad_request() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "invalid_visual_diff_evidence"})),
    )
        .into_response()
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

fn valid_viewport(viewport: &VisualViewport) -> bool {
    viewport.css_width > 0
        && viewport.css_height > 0
        && viewport.device_scale_factor.is_finite()
        && viewport.device_scale_factor > 0.0
        && viewport.device_scale_factor <= 8.0
}

fn valid_unit_ratio(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn coherent_shape(request: &VisualDiffEvidenceRequest) -> bool {
    if request.visual_evidence_ids.len() > MAX_VISUAL_DIFF_PARENTS
        || !valid_unit_ratio(request.changed_ratio)
    {
        return false;
    }

    let unique_parents = request
        .visual_evidence_ids
        .iter()
        .collect::<HashSet<_>>()
        .len()
        == request.visual_evidence_ids.len();
    if !unique_parents {
        return false;
    }

    match request.mode {
        VisualDiffMode::Unchanged => {
            request.changed_ratio == 0.0 && request.visual_evidence_ids.is_empty()
        }
        VisualDiffMode::BaselineReset => {
            request.changed_ratio == 1.0 && request.visual_evidence_ids.len() == 1
        }
        VisualDiffMode::Viewport => {
            request.changed_ratio > 0.0 && request.visual_evidence_ids.len() == 1
        }
        VisualDiffMode::Regions => {
            request.changed_ratio > 0.0 && !request.visual_evidence_ids.is_empty()
        }
    }
}

async fn parents_are_correlated(
    state: &ControlState,
    session_id: SessionId,
    request: &VisualDiffEvidenceRequest,
    canonical_route: &str,
) -> bool {
    let Some(expected_target) = request.mode.expected_parent_target() else {
        return request.visual_evidence_ids.is_empty();
    };

    for parent_id in &request.visual_evidence_ids {
        let Some(parent) = state.evidence.get(parent_id).await else {
            return false;
        };
        if parent.kind != EvidenceKind::Visual
            || parent.session_id != session_id
            || parent.provenance.revision != request.revision
            || parent.payload.get("target").and_then(|value| value.as_str()) != Some(expected_target)
        {
            return false;
        }

        let Some(parent_route) = parent.payload.get("route").and_then(|value| value.as_str()) else {
            return false;
        };
        if canonical_loopback_route(parent_route).as_deref() != Some(canonical_route) {
            return false;
        }

        let Some(parent_viewport) = parent.payload.get("viewport") else {
            return false;
        };
        let Ok(parent_viewport) = serde_json::from_value::<VisualViewport>(parent_viewport.clone()) else {
            return false;
        };
        if parent_viewport != request.viewport {
            return false;
        }
    }

    true
}

async fn ingest_visual_diff_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<VisualDiffEvidenceRequest>,
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

    let Some(canonical_route) = canonical_loopback_route(&request.route) else {
        return bad_request();
    };
    if request.captured_at_unix_ms < 0 || !valid_viewport(&request.viewport) || !coherent_shape(&request)
    {
        return bad_request();
    }
    let Some(captured_at) = Utc.timestamp_millis_opt(request.captured_at_unix_ms).single() else {
        return bad_request();
    };
    if !parents_are_correlated(&state, id, &request, &canonical_route).await {
        return bad_request();
    }

    let payload = serde_json::json!({
        "route": canonical_route,
        "viewport": request.viewport,
        "mode": request.mode.as_str(),
        "changed_ratio": request.changed_ratio,
        "baseline_comparable": request.mode.baseline_comparable(),
    });
    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Contract,
            session_id: id,
            region: Some("viewport".into()),
            payload,
            provenance: Provenance {
                source: "native-visual-diff".into(),
                engine: Some("pixel-diff".into()),
                revision: request.revision,
                parent_ids: request.visual_evidence_ids,
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
