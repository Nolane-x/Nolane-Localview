use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use localview_capture::{evaluate_settle, SettleObservation, StableCapturePolicy};
use localview_live_bridge::{
    BridgeActionKind, BridgeActionResult, ObserverEvent, ObserverEventKind,
};
use localview_protocol::SessionId;
use serde::Deserialize;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::ControlState;

const FRESH_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(650);
const VISUAL_STATE_TIMEOUT: Duration = Duration::from_millis(1_200);
const ACTION_RESULT_POLL: Duration = Duration::from_millis(20);
const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;
const MAX_INTERNAL_CAPTURE_ACTION_DRAIN: usize = 16;

#[derive(Debug, Clone, Copy)]
enum ActionResultScope {
    Public,
    InternalCapture,
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/capture-settle",
            get(session_capture_settle),
        )
        .route(
            "/v1/sessions/{id}/capture-actions",
            get(session_capture_actions),
        )
        .route(
            "/v1/sessions/{id}/capture-freeze",
            post(session_capture_freeze),
        )
        .route(
            "/v1/sessions/{id}/capture-restore",
            post(session_capture_restore),
        )
        .with_state(state)
}

async fn session_capture_settle(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }

    let snapshot_action = state
        .live
        .enqueue_action(id, None, BridgeActionKind::Snapshot)
        .await;
    let fresh_snapshot = wait_for_action_result(
        &state,
        id,
        snapshot_action.id,
        FRESH_SNAPSHOT_TIMEOUT,
        ActionResultScope::Public,
    )
    .await
    .filter(|result| result.ok);
    let events = state.live.recent(id, 2048).await;
    let observation = settle_observation(
        &events,
        fresh_snapshot.as_ref(),
        Utc::now().timestamp_millis(),
    );
    Json(evaluate_settle(
        &StableCapturePolicy::default(),
        &observation,
    ))
    .into_response()
}

async fn session_capture_actions(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }

    Json(
        state
            .live
            .take_internal_capture_actions(id, MAX_INTERNAL_CAPTURE_ACTION_DRAIN)
            .await,
    )
    .into_response()
}

async fn session_capture_freeze(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }

    let action = state
        .live
        .enqueue_action(id, None, BridgeActionKind::FreezeVisuals)
        .await;
    let Some(result) = wait_for_action_result(
        &state,
        id,
        action.id,
        VISUAL_STATE_TIMEOUT,
        ActionResultScope::InternalCapture,
    )
    .await
    else {
        return bounded_error(StatusCode::GATEWAY_TIMEOUT, "visual_freeze_ack_timeout");
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "visual_freeze_failed");
    }

    let paused_animations = result
        .payload
        .get("paused_animations")
        .and_then(serde_json::Value::as_u64);
    let web_animations_supported = result
        .payload
        .get("web_animations_supported")
        .and_then(serde_json::Value::as_bool);
    let (Some(paused_animations), Some(web_animations_supported)) =
        (paused_animations, web_animations_supported)
    else {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_visual_freeze_ack");
    };
    if paused_animations > MAX_PAUSED_ANIMATIONS {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_visual_freeze_ack");
    }

    Json(serde_json::json!({
        "token": action.id,
        "paused_animations": paused_animations,
        "web_animations_supported": web_animations_supported,
        "lease_ms": VISUAL_FREEZE_LEASE_MS,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRestoreRequest {
    token: Uuid,
}

async fn session_capture_restore(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<CaptureRestoreRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }

    let action = state
        .live
        .enqueue_action(
            id,
            None,
            BridgeActionKind::RestoreVisuals {
                token: request.token,
            },
        )
        .await;
    let Some(result) = wait_for_action_result(
        &state,
        id,
        action.id,
        VISUAL_STATE_TIMEOUT,
        ActionResultScope::InternalCapture,
    )
    .await
    else {
        return bounded_error(StatusCode::GATEWAY_TIMEOUT, "visual_restore_ack_timeout");
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "visual_restore_failed");
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn wait_for_action_result(
    state: &ControlState,
    session_id: SessionId,
    action_id: Uuid,
    timeout: Duration,
    scope: ActionResultScope,
) -> Option<BridgeActionResult> {
    let deadline = Instant::now() + timeout;
    loop {
        let results = match scope {
            ActionResultScope::Public => state.live.recent_results(session_id, 64).await,
            ActionResultScope::InternalCapture => {
                state
                    .live
                    .recent_internal_capture_results(session_id, 64)
                    .await
            }
        };
        if let Some(result) = results
            .into_iter()
            .rev()
            .find(|result| result.action_id == action_id)
        {
            return Some(result);
        }

        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        sleep(ACTION_RESULT_POLL.min(deadline.saturating_duration_since(now))).await;
    }
}

async fn session_exists(state: &ControlState, id: SessionId) -> bool {
    state.sessions.get(id).await.is_some()
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn denied() -> axum::response::Response {
    bounded_error(StatusCode::UNAUTHORIZED, "unauthorized")
}

fn session_not_found() -> axum::response::Response {
    bounded_error(StatusCode::NOT_FOUND, "session_not_found")
}

fn bounded_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

fn settle_observation(
    events: &[ObserverEvent],
    fresh_snapshot: Option<&BridgeActionResult>,
    now_unix_ms: i64,
) -> SettleObservation {
    let snapshot = fresh_snapshot.map(|result| &result.payload);
    let latest_semantic_at_unix_ms = fresh_snapshot.map(|_| now_unix_ms);
    let ready_state = snapshot
        .and_then(|value| value.get("readyState"))
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let fonts_status = snapshot
        .and_then(|value| value.get("readiness"))
        .and_then(|value| value.get("fonts"))
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let pending_images = snapshot
        .and_then(|value| value.get("readiness"))
        .and_then(|value| value.get("pendingImages"))
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok());

    let mut latest_hmr_at_unix_ms = None;
    let mut latest_dom_mutation_at_unix_ms = None;
    let mut latest_layout_at_unix_ms = None;
    let mut latest_network_at_unix_ms = None;

    for event in events {
        let captured_at = event.captured_at.timestamp_millis();
        match event.kind {
            ObserverEventKind::Hmr => update_latest(&mut latest_hmr_at_unix_ms, captured_at),
            ObserverEventKind::DomMutation => {
                update_latest(&mut latest_dom_mutation_at_unix_ms, captured_at)
            }
            ObserverEventKind::Layout => update_latest(&mut latest_layout_at_unix_ms, captured_at),
            ObserverEventKind::Network => update_latest(&mut latest_network_at_unix_ms, captured_at),
            ObserverEventKind::SemanticSnapshot
            | ObserverEventKind::Route
            | ObserverEventKind::Focus
            | ObserverEventKind::Scroll
            | ObserverEventKind::Console
            | ObserverEventKind::RuntimeError
            | ObserverEventKind::Performance => {}
        }
    }

    SettleObservation {
        now_unix_ms,
        latest_semantic_at_unix_ms,
        ready_state,
        fonts_status,
        pending_images,
        latest_hmr_at_unix_ms,
        latest_dom_mutation_at_unix_ms,
        latest_layout_at_unix_ms,
        latest_network_at_unix_ms,
    }
}

fn update_latest(slot: &mut Option<i64>, candidate: i64) {
    if slot.is_none_or(|current| candidate > current) {
        *slot = Some(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn observation_reads_only_fresh_bounded_readiness_fields() {
        let stale_event = ObserverEvent {
            seq: 1,
            captured_at: Utc.timestamp_millis_opt(1_000).single().unwrap(),
            kind: ObserverEventKind::SemanticSnapshot,
            reference: None,
            route: None,
            payload: serde_json::json!({
                "snapshot": {
                    "readyState": "complete",
                    "readiness": {"fonts": "loaded", "pendingImages": 0},
                    "secret": "stale-and-ignored"
                }
            }),
        };
        let fresh = BridgeActionResult {
            action_id: Uuid::nil(),
            ok: true,
            error: None,
            payload: serde_json::json!({
                "readyState": "interactive",
                "readiness": {"fonts": "loading", "pendingImages": 2},
                "secret": "never-copied"
            }),
            completed_at: Utc.timestamp_millis_opt(99_000).single().unwrap(),
        };

        let observation = settle_observation(&[stale_event], Some(&fresh), 2_000);
        assert_eq!(observation.latest_semantic_at_unix_ms, Some(2_000));
        assert_eq!(observation.ready_state.as_deref(), Some("interactive"));
        assert_eq!(observation.fonts_status.as_deref(), Some("loading"));
        assert_eq!(observation.pending_images, Some(2));
    }
}
