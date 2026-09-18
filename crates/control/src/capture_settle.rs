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
use serde_json::Value;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::ControlState;

const FRESH_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(650);
const VISUAL_STATE_TIMEOUT: Duration = Duration::from_millis(1_200);
const ACTION_RESULT_POLL: Duration = Duration::from_millis(20);
const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;
const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;
const MAX_POSITIONAL_SCAN_ELEMENTS: u64 = 4_096;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;
const MAX_VISUAL_MASK_RECTS: usize = 256;
const MAX_MASKED_ELEMENTS: u64 = 4_096;
const MAX_CSS_VIEWPORT_DIMENSION: f64 = 100_000.0;
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
        .route(
            "/v1/sessions/{id}/capture-freeze-full-page",
            post(session_capture_freeze_full_page),
        )
        .route(
            "/v1/sessions/{id}/capture-scroll",
            post(session_capture_scroll),
        )
        .route(
            "/v1/sessions/{id}/capture-tile-probe",
            post(session_capture_tile_probe),
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
        .enqueue_capture_freeze(id, StableCapturePolicy::default().mask_selectors)
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
        .and_then(Value::as_u64);
    let web_animations_supported = result
        .payload
        .get("web_animations_supported")
        .and_then(Value::as_bool);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);
    let masked_elements = result
        .payload
        .get("masked_elements")
        .and_then(Value::as_u64);
    let mask_rects = validated_mask_rects(&result.payload);

    let valid_viewport = viewport_css_width.is_some_and(|value| {
        value.is_finite() && value > 0.0 && value <= MAX_CSS_VIEWPORT_DIMENSION
    }) && viewport_css_height.is_some_and(|value| {
        value.is_finite() && value > 0.0 && value <= MAX_CSS_VIEWPORT_DIMENSION
    });
    let valid_counts = paused_animations.is_some_and(|value| value <= MAX_PAUSED_ANIMATIONS)
        && web_animations_supported.is_some()
        && masked_elements.is_some_and(|value| value <= MAX_MASKED_ELEMENTS)
        && mask_rects.is_some();
    if !valid_viewport || !valid_counts {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_visual_freeze_ack");
    }

    Json(serde_json::json!({
        "token": action.id,
        "paused_animations": paused_animations.expect("validated above"),
        "web_animations_supported": web_animations_supported.expect("validated above"),
        "viewport_css_width": viewport_css_width.expect("validated above"),
        "viewport_css_height": viewport_css_height.expect("validated above"),
        "masked_elements": masked_elements.expect("validated above"),
        "mask_rects": mask_rects.expect("validated above"),
        "lease_ms": VISUAL_FREEZE_LEASE_MS,
    }))
    .into_response()
}


async fn session_capture_freeze_full_page(
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
        .enqueue_full_page_capture_freeze(id, StableCapturePolicy::default().mask_selectors)
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
        return bounded_error(
            StatusCode::GATEWAY_TIMEOUT,
            "full_page_visual_freeze_ack_timeout",
        );
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "full_page_visual_freeze_failed");
    }

    let paused_animations = result
        .payload
        .get("paused_animations")
        .and_then(Value::as_u64);
    let web_animations_supported = result
        .payload
        .get("web_animations_supported")
        .and_then(Value::as_bool);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);
    let masked_elements = result
        .payload
        .get("masked_elements")
        .and_then(Value::as_u64);
    let mask_rects = validated_mask_rects(&result.payload);
    let scroll_x = result.payload.get("scroll_x").and_then(Value::as_f64);
    let scroll_y = result.payload.get("scroll_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);

    let valid = paused_animations.is_some_and(|value| value <= MAX_PAUSED_ANIMATIONS)
        && web_animations_supported.is_some()
        && viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension)
        && masked_elements.is_some_and(|value| value <= MAX_MASKED_ELEMENTS)
        && mask_rects.is_some()
        && scroll_x.is_some_and(valid_nonnegative_css_coordinate)
        && scroll_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height);

    if !valid {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_full_page_visual_freeze_ack");
    }

    let viewport_css_width = viewport_css_width.expect("validated above");
    let viewport_css_height = viewport_css_height.expect("validated above");
    let document_css_width = document_css_width.expect("validated above");
    let document_css_height = document_css_height.expect("validated above");
    let scroll_x = scroll_x.expect("validated above");
    let scroll_y = scroll_y.expect("validated above");
    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    if document_css_width != viewport_css_width
        || scroll_x > 0.5
        || scroll_y > max_scroll_y + 0.5
    {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_full_page_visual_freeze_ack");
    }

    Json(serde_json::json!({
        "token": action.id,
        "paused_animations": paused_animations.expect("validated above"),
        "web_animations_supported": web_animations_supported.expect("validated above"),
        "viewport_css_width": viewport_css_width,
        "viewport_css_height": viewport_css_height,
        "masked_elements": masked_elements.expect("validated above"),
        "mask_rects": mask_rects.expect("validated above"),
        "scroll_x": scroll_x,
        "scroll_y": scroll_y,
        "document_css_width": document_css_width,
        "document_css_height": document_css_height,
        "lease_ms": FULL_PAGE_VISUAL_FREEZE_LEASE_MS,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureScrollRequest {
    token: Uuid,
    y: f64,
}

async fn session_capture_scroll(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<CaptureScrollRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }
    if !valid_full_page_y(request.y) {
        return bounded_error(StatusCode::BAD_REQUEST, "invalid_capture_scroll_request");
    }

    let action = state
        .live
        .enqueue_action(
            id,
            None,
            BridgeActionKind::CaptureScrollTo {
                token: request.token,
                y: request.y,
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
        return bounded_error(StatusCode::GATEWAY_TIMEOUT, "capture_scroll_ack_timeout");
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "capture_scroll_failed");
    }

    let requested_y = result.payload.get("requested_y").and_then(Value::as_f64);
    let actual_x = result.payload.get("actual_x").and_then(Value::as_f64);
    let actual_y = result.payload.get("actual_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);

    let valid = requested_y.is_some_and(valid_full_page_y)
        && actual_x.is_some_and(valid_nonnegative_css_coordinate)
        && actual_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height)
        && viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension);
    if !valid {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_capture_scroll_ack");
    }

    let requested_y = requested_y.expect("validated above");
    let actual_x = actual_x.expect("validated above");
    let actual_y = actual_y.expect("validated above");
    let document_css_width = document_css_width.expect("validated above");
    let document_css_height = document_css_height.expect("validated above");
    let viewport_css_width = viewport_css_width.expect("validated above");
    let viewport_css_height = viewport_css_height.expect("validated above");
    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    if requested_y != request.y
        || document_css_width != viewport_css_width
        || requested_y > max_scroll_y + 0.5
        || actual_y > max_scroll_y + 0.5
        || (actual_y - requested_y).abs() > 0.5
        || actual_x > 0.5
    {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_capture_scroll_ack");
    }

    Json(serde_json::json!({
        "requested_y": requested_y,
        "actual_x": actual_x,
        "actual_y": actual_y,
        "document_css_width": document_css_width,
        "document_css_height": document_css_height,
        "viewport_css_width": viewport_css_width,
        "viewport_css_height": viewport_css_height,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureTileProbeRequest {
    token: Uuid,
}

async fn session_capture_tile_probe(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<CaptureTileProbeRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if !session_exists(&state, id).await {
        return session_not_found();
    }

    let action = state
        .live
        .enqueue_capture_tile_probe(
            id,
            request.token,
            StableCapturePolicy::default().mask_selectors,
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
        return bounded_error(StatusCode::GATEWAY_TIMEOUT, "capture_tile_probe_ack_timeout");
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "capture_tile_probe_failed");
    }

    let scroll_x = result.payload.get("scroll_x").and_then(Value::as_f64);
    let scroll_y = result.payload.get("scroll_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);
    let masked_elements = result
        .payload
        .get("masked_elements")
        .and_then(Value::as_u64);
    let mask_rects = validated_mask_rects(&result.payload);
    let positional_elements_scanned = result
        .payload
        .get("positional_elements_scanned")
        .and_then(Value::as_u64);
    let visible_fixed_or_sticky = result
        .payload
        .get("visible_fixed_or_sticky")
        .and_then(Value::as_bool);

    let valid = scroll_x.is_some_and(valid_nonnegative_css_coordinate)
        && scroll_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height)
        && viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension)
        && masked_elements.is_some_and(|value| value <= MAX_MASKED_ELEMENTS)
        && mask_rects.is_some()
        && positional_elements_scanned.is_some_and(|value| value <= MAX_POSITIONAL_SCAN_ELEMENTS)
        && visible_fixed_or_sticky.is_some();
    if !valid {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_capture_tile_probe_ack");
    }

    let scroll_x = scroll_x.expect("validated above");
    let scroll_y = scroll_y.expect("validated above");
    let document_css_width = document_css_width.expect("validated above");
    let document_css_height = document_css_height.expect("validated above");
    let viewport_css_width = viewport_css_width.expect("validated above");
    let viewport_css_height = viewport_css_height.expect("validated above");
    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    if document_css_width != viewport_css_width
        || scroll_x > 0.5
        || scroll_y > max_scroll_y + 0.5
    {
        return bounded_error(StatusCode::BAD_GATEWAY, "invalid_capture_tile_probe_ack");
    }

    Json(serde_json::json!({
        "scroll_x": scroll_x,
        "scroll_y": scroll_y,
        "document_css_width": document_css_width,
        "document_css_height": document_css_height,
        "viewport_css_width": viewport_css_width,
        "viewport_css_height": viewport_css_height,
        "masked_elements": masked_elements.expect("validated above"),
        "mask_rects": mask_rects.expect("validated above"),
        "positional_elements_scanned": positional_elements_scanned.expect("validated above"),
        "visible_fixed_or_sticky": visible_fixed_or_sticky.expect("validated above"),
    }))
    .into_response()
}

fn valid_positive_css_dimension(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_CSS_VIEWPORT_DIMENSION
}

fn valid_positive_document_height(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT
}

fn valid_nonnegative_css_coordinate(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_CSS_VIEWPORT_DIMENSION).contains(&value)
}

fn valid_full_page_y(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT).contains(&value)
}

fn validated_mask_rects(payload: &Value) -> Option<Vec<Value>> {
    let rects = payload.get("mask_rects")?.as_array()?;
    if rects.len() > MAX_VISUAL_MASK_RECTS {
        return None;
    }

    let mut out = Vec::with_capacity(rects.len());
    for rect in rects {
        let x = rect.get("x")?.as_f64()?;
        let y = rect.get("y")?.as_f64()?;
        let width = rect.get("width")?.as_f64()?;
        let height = rect.get("height")?.as_f64()?;
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || !(x + width).is_finite()
            || !(y + height).is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            return None;
        }
        out.push(serde_json::json!({
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }));
    }
    Some(out)
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
    let inflight_network_requests = snapshot
        .and_then(|value| value.get("readiness"))
        .and_then(|value| value.get("inflightRequests"))
        .and_then(Value::as_u64)
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
        inflight_network_requests,
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
                    "readiness": {"fonts": "loaded", "pendingImages": 0, "inflightRequests": 99},
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
                "readiness": {"fonts": "loading", "pendingImages": 2, "inflightRequests": 3},
                "secret": "never-copied"
            }),
            completed_at: Utc.timestamp_millis_opt(99_000).single().unwrap(),
        };

        let observation = settle_observation(&[stale_event], Some(&fresh), 2_000);
        assert_eq!(observation.latest_semantic_at_unix_ms, Some(2_000));
        assert_eq!(observation.ready_state.as_deref(), Some("interactive"));
        assert_eq!(observation.fonts_status.as_deref(), Some("loading"));
        assert_eq!(observation.pending_images, Some(2));
        assert_eq!(observation.inflight_network_requests, Some(3));
    }
}
