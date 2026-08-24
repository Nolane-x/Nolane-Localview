use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use localview_capture::{evaluate_settle, SettleObservation, StableCapturePolicy};
use localview_live_bridge::{
    BridgeActionKind, BridgeActionResult, ObserverEvent, ObserverEventKind,
};
use localview_protocol::SessionId;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::ControlState;

const FRESH_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(650);
const FRESH_SNAPSHOT_POLL: Duration = Duration::from_millis(20);

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/capture-settle",
            get(session_capture_settle),
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
    if state.sessions.get(id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response();
    }

    let snapshot_action = state
        .live
        .enqueue_action(id, None, BridgeActionKind::Snapshot)
        .await;
    let fresh_snapshot = wait_for_snapshot_result(&state, id, snapshot_action.id).await;
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

async fn wait_for_snapshot_result(
    state: &ControlState,
    session_id: SessionId,
    action_id: Uuid,
) -> Option<BridgeActionResult> {
    let deadline = Instant::now() + FRESH_SNAPSHOT_TIMEOUT;
    loop {
        if let Some(result) = state
            .live
            .recent_results(session_id, 64)
            .await
            .into_iter()
            .rev()
            .find(|result| result.action_id == action_id)
        {
            return result.ok.then_some(result);
        }

        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        sleep(FRESH_SNAPSHOT_POLL.min(deadline.saturating_duration_since(now))).await;
    }
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn denied() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
        .into_response()
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
