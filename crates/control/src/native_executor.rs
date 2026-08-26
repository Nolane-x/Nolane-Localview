#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use localview_evidence::{EvidenceKind, EvidenceObject, UncertaintyClass};
use localview_live_bridge::{NativeExecutorAction, NativeExecutorRequest, NativeExecutorResult};
use localview_protocol::{SessionId, ViewportMeta};
use serde_json::Value;

use crate::{
    perception::{authorized, denied},
    ControlState,
};

const MAX_NATIVE_EXECUTOR_POLL_BATCH: usize = 8;
const NATIVE_EXECUTOR_ACTIVE_LEASE_SECS: i64 = 15;
const MAX_NATIVE_EXECUTOR_EVIDENCE_IDS: usize = 8;
const NATIVE_VISUAL_EVIDENCE_CORRELATION_ERROR: &str =
    "native visual evidence correlation failed";

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
    Json(mut result): Json<NativeExecutorResult>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    let Some(origin) = state
        .live
        .claim_native_executor(id, result.request_id)
        .await
    else {
        return result_without_origin();
    };

    if result.ok && !native_result_evidence_correlated(&state, &origin, &result).await {
        result.ok = false;
        result.usage = None;
        result.payload = Value::Null;
        result.error = Some(NATIVE_VISUAL_EVIDENCE_CORRELATION_ERROR.into());
    }

    if !state.live.complete_native_executor(id, result).await {
        return result_without_origin();
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn native_result_evidence_correlated(
    state: &ControlState,
    request: &NativeExecutorRequest,
    result: &NativeExecutorResult,
) -> bool {
    match &request.action {
        NativeExecutorAction::VisualPacket {
            viewport, revision, ..
        } => {
            let Some(usage) = result.usage.as_ref() else {
                return false;
            };
            let Some(evidence_ids) = result.payload.get("evidence_ids").and_then(Value::as_array)
            else {
                return false;
            };
            if evidence_ids.is_empty()
                || evidence_ids.len() > MAX_NATIVE_EXECUTOR_EVIDENCE_IDS
                || evidence_ids.len() != usage.image_regions
            {
                return false;
            }

            for evidence_id in evidence_ids {
                let Some(evidence_id) = evidence_id.as_str() else {
                    return false;
                };
                let Some(evidence) = state.evidence.get(evidence_id).await else {
                    return false;
                };
                if !authoritative_native_visual_evidence(
                    &evidence,
                    request,
                    result,
                    viewport,
                    revision.as_deref(),
                ) {
                    return false;
                }
            }
            true
        }
    }
}

fn authoritative_native_visual_evidence(
    evidence: &EvidenceObject,
    request: &NativeExecutorRequest,
    result: &NativeExecutorResult,
    viewport: &ViewportMeta,
    revision: Option<&str>,
) -> bool {
    if evidence.kind != EvidenceKind::Visual
        || evidence.session_id != request.session_id
        || evidence.provenance.source != "native-capture"
        || evidence.uncertainty != UncertaintyClass::Observed
        || evidence.confidence < 0.999
        || evidence.secret_taint
        || evidence.provenance.captured_at < request.created_at
        || evidence.provenance.captured_at > result.completed_at
        || !matches!(
            evidence.provenance.engine.as_deref(),
            Some("webview2" | "wk_web_view" | "web_kit_gtk")
        )
    {
        return false;
    }
    if revision.is_some() && evidence.provenance.revision.as_deref() != revision {
        return false;
    }

    let Some(evidence_viewport) = evidence.payload.get("viewport") else {
        return false;
    };
    let width_matches = evidence_viewport
        .get("css_width")
        .and_then(Value::as_u64)
        == Some(u64::from(viewport.css_width));
    let height_matches = evidence_viewport
        .get("css_height")
        .and_then(Value::as_u64)
        == Some(u64::from(viewport.css_height));
    let scale_matches = evidence_viewport
        .get("device_scale_factor")
        .and_then(Value::as_f64)
        .is_some_and(|scale| (scale - viewport.device_scale_factor).abs() <= f64::EPSILON);
    width_matches && height_matches && scale_matches
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
