#[test]
fn viewport_capture_serializes_per_session_and_orders_visual_transaction() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("MAX_CAPTURE_SESSION_GATES"));
    assert!(module.contains("capture_gates"));
    assert!(module.contains("session_capture_gate"));
    assert!(module.contains("Weak<Mutex<()>>"));
    assert!(!module.contains("capture_gate: Mutex<()>"));
    assert!(!module.contains("global_capture_mutex"));

    let helper_start = module
        .find("async fn capture_redacted_viewport_after_gate(")
        .expect("shared redacted viewport transaction must exist");
    let helper_end = module[helper_start..]
        .find("\nasync fn capture_target(")
        .map(|offset| helper_start + offset)
        .expect("shared redacted viewport helper must end before target processing");
    let helper = &module[helper_start..helper_end];

    let settle = helper
        .find("wait_for_capture_settle(session_id).await?")
        .expect("capture must settle before freezing");
    let freeze = helper
        .find("freeze_visual_state(session_id).await?")
        .expect("capture must freeze visual motion");
    let pixels = helper
        .find("capture_managed_surface(&app, session_id, viewport, revision).await")
        .expect("native capture attempt must remain explicit");
    let restore = helper
        .find("restore_visual_state(session_id, &freeze.token).await")
        .expect("capture must restore using the exact freeze token");
    let redact = helper
        .find("redact_private_pixels(frame, &freeze)?")
        .expect("private masks must redact pixels before returning the frame");

    assert_eq!(helper.matches("capture_managed_surface(").count(), 1);
    assert!(settle < freeze);
    assert!(freeze < pixels);
    assert!(pixels < restore);
    assert!(restore < redact);

    let target_start = module
        .find("async fn capture_target(")
        .expect("shared target transaction must exist");
    let target_end = module[target_start..]
        .find("async fn compatible_changed_baseline(")
        .map(|offset| target_start + offset)
        .expect("target transaction must end before baseline helpers");
    let transaction = &module[target_start..target_end];

    let gate = transaction
        .find("session_capture_gate(&state, session_id).await?")
        .expect("capture must acquire a session-scoped gate");
    let shared = transaction
        .find("capture_redacted_viewport_after_gate(")
        .expect("target capture must use the shared redacted transaction");
    let consistency = transaction
        .find("validate_live_target_viewport(&frame, &freeze, &target)?")
        .expect("region target must verify live viewport geometry");
    let target = transaction
        .find("apply_capture_target(frame, &freeze, &target)?")
        .expect("capture must process the bounded target before persistence");
    let persist = transaction
        .find("persist_and_register(&state, session_id, frame, &target).await")
        .expect("pixels may only be persisted after shared restoration/redaction and validation");

    assert!(gate < shared);
    assert!(shared < consistency);
    assert!(consistency < target);
    assert!(target < persist);
}

#[test]
fn restore_is_attempted_after_native_failure_and_failure_discards_pixels() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("let native_result = capture_managed_surface("));
    assert!(module.contains(
        "let restore_result = restore_visual_state(session_id, &freeze.token).await;"
    ));
    assert!(module.contains("visual capture restore acknowledgement failed; pixels discarded"));
    assert!(module.contains("match (native_result, restore_result)"));
    assert!(module.contains(
        "(Err(native_error), Ok(())) => return Err(native_error)"
    ));
    assert!(module.contains("(Ok(_), Err(_)) | (Err(_), Err(_))"));

    assert!(!module.contains("eval("));
    assert!(!module.contains("html2canvas"));
    assert!(!module.contains("canvas.toDataURL"));
}

#[test]
fn visual_state_control_receipts_are_private_and_bounded() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("/capture-freeze"));
    assert!(module.contains("/capture-restore"));
    assert!(module.contains("FreezeVisualStateReceipt"));
    assert!(module.contains("lease_ms"));
    assert!(module.contains("paused_animations"));
    assert!(module.contains("web_animations_supported"));
    assert!(module.contains("viewport_css_width"));
    assert!(module.contains("viewport_css_height"));
    assert!(module.contains("masked_elements"));
    assert!(module.contains("mask_rects"));
    assert!(module.contains("MAX_VISUAL_MASK_RECTS"));
    assert!(module.contains("MAX_MASKED_ELEMENTS"));
    assert!(!module.contains("pub token:"));
    assert!(!module.contains("freeze_token"));
}

#[test]
fn private_mask_redaction_reuses_bounded_visual_core_and_fails_closed() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("localview_visual::redact_png_css_rects"));
    assert!(module.contains("frame.pixel_width"));
    assert!(module.contains("frame.pixel_height"));
    assert!(module.contains("freeze.viewport_css_width"));
    assert!(module.contains("freeze.viewport_css_height"));
    assert!(module.contains("&freeze.mask_rects"));
    assert!(module.contains("private visual mask redaction failed; pixels discarded"));
    assert!(module.contains("private visual mask application was incomplete; pixels discarded"));
}

#[test]
fn region_capture_rejects_live_viewport_drift_before_persistence() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("validate_live_target_viewport"));
    assert!(module.contains("native visual region viewport changed during capture; pixels discarded"));
    assert!(module.contains("frame.viewport.css_width"));
    assert!(module.contains("frame.viewport.css_height"));
    assert!(module.contains("freeze.viewport_css_width"));
    assert!(module.contains("freeze.viewport_css_height"));
}
