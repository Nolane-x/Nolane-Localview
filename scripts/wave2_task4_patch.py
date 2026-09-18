from pathlib import Path

def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)

path = Path("crates/control/src/capture_settle.rs")
text = path.read_text()

text = replace_once(
    text,
    "const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;\n",
    "const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;\nconst FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;\nconst MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;\nconst MAX_POSITIONAL_SCAN_ELEMENTS: u64 = 4_096;\n",
    "full-page control constants",
)

text = replace_once(
    text,
    '''        .route(
            "/v1/sessions/{id}/capture-restore",
            post(session_capture_restore),
        )
        .with_state(state)
''',
    '''        .route(
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
''',
    "full-page capture routes",
)

marker = "fn validated_mask_rects(payload: &Value) -> Option<Vec<Value>> {"
insert = r'''
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

'''
if text.count(marker) != 1:
    raise SystemExit("validated_mask_rects insertion marker mismatch")
text = text.replace(marker, insert + marker, 1)
path.write_text(text)

runtime_path = Path("crates/control/src/runtime.rs")
runtime = runtime_path.read_text()
runtime = replace_once(
    runtime,
    '''    let Some(action) = state.live.claim_action(id, result.action_id).await else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "action_result_without_inflight_origin"})),
        )
            .into_response();
    };
    let revision = state
''',
    '''    let Some(action) = state.live.claim_action(id, result.action_id).await else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "action_result_without_inflight_origin"})),
        )
            .into_response();
    };
    if action.action.is_internal_capture_action() {
        state.live.complete_action(&action, result).await;
        return StatusCode::NO_CONTENT.into_response();
    }
    let revision = state
''',
    "internal capture evidence fence",
)
runtime_path.write_text(runtime)
