#[test]
fn viewport_capture_serializes_per_session_and_orders_visual_transaction() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("MAX_CAPTURE_SESSION_GATES"));
    assert!(module.contains("capture_gates"));
    assert!(module.contains("session_capture_gate"));
    assert!(module.contains("Weak<Mutex<()>>"));
    assert!(!module.contains("capture_gate: Mutex<()>"));
    assert!(!module.contains("global_capture_mutex"));

    let gate = module
        .find("session_capture_gate(&state, session_id).await?")
        .expect("capture must acquire a session-scoped gate");
    let settle = module
        .find("wait_for_capture_settle(session_id).await?")
        .expect("capture must settle before freezing");
    let freeze = module
        .find("freeze_visual_state(session_id).await?")
        .expect("capture must freeze visual motion");
    let pixels = module
        .find("capture_managed_surface(&app, session_id, viewport, revision).await")
        .expect("native capture attempt must remain explicit");
    let restore = module
        .find("restore_visual_state(session_id, &freeze.token).await")
        .expect("capture must restore using the exact freeze token");
    let persist = module
        .find("persist_and_register(&state, session_id, frame).await")
        .expect("pixels may only be persisted after restore");

    assert!(gate < settle);
    assert!(settle < freeze);
    assert!(freeze < pixels);
    assert!(pixels < restore);
    assert!(restore < persist);
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
    assert!(!module.contains("pub token:"));
    assert!(!module.contains("freeze_token"));
}
