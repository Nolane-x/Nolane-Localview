use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Utc;
use localview_capture::{evaluate_settle, SettleObservation, StableCapturePolicy};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_protocol::SessionId;

use crate::ControlState;

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

    let events = state.live.recent(id, 2048).await;
    let observation = settle_observation(&events, Utc::now().timestamp_millis());
    Json(evaluate_settle(
        &StableCapturePolicy::default(),
        &observation,
    ))
    .into_response()
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

fn settle_observation(events: &[ObserverEvent], now_unix_ms: i64) -> SettleObservation {
    let mut latest_semantic_at_unix_ms = None;
    let mut ready_state = None;
    let mut fonts_status = None;
    let mut pending_images = None;
    let mut latest_hmr_at_unix_ms = None;
    let mut latest_dom_mutation_at_unix_ms = None;
    let mut latest_layout_at_unix_ms = None;
    let mut latest_network_at_unix_ms = None;

    for event in events {
        let captured_at = event.captured_at.timestamp_millis();
        match event.kind {
            ObserverEventKind::SemanticSnapshot => {
                latest_semantic_at_unix_ms = Some(captured_at);
                let snapshot = event.payload.get("snapshot");
                ready_state = snapshot
                    .and_then(|value| value.get("readyState"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned);
                fonts_status = snapshot
                    .and_then(|value| value.get("readiness"))
                    .and_then(|value| value.get("fonts"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned);
                pending_images = snapshot
                    .and_then(|value| value.get("readiness"))
                    .and_then(|value| value.get("pendingImages"))
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok());
            }
            ObserverEventKind::Hmr => latest_hmr_at_unix_ms = Some(captured_at),
            ObserverEventKind::DomMutation => latest_dom_mutation_at_unix_ms = Some(captured_at),
            ObserverEventKind::Layout => latest_layout_at_unix_ms = Some(captured_at),
            ObserverEventKind::Network => latest_network_at_unix_ms = Some(captured_at),
            ObserverEventKind::Route
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn observation_reads_only_bounded_readiness_fields() {
        let event = ObserverEvent {
            seq: 1,
            captured_at: Utc.timestamp_millis_opt(1_000).single().unwrap(),
            kind: ObserverEventKind::SemanticSnapshot,
            reference: None,
            route: None,
            payload: serde_json::json!({
                "snapshot": {
                    "readyState": "complete",
                    "readiness": {"fonts": "loaded", "pendingImages": 0},
                    "secret": "ignored"
                }
            }),
        };
        let observation = settle_observation(&[event], 2_000);
        assert_eq!(observation.ready_state.as_deref(), Some("complete"));
        assert_eq!(observation.fonts_status.as_deref(), Some("loaded"));
        assert_eq!(observation.pending_images, Some(0));
    }
}
