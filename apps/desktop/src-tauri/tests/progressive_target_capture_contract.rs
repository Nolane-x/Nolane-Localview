#[test]
fn progressive_target_capture_is_registered_fresh_and_exact_level_only() {
    let lib = include_str!("../src/lib.rs");
    let module = include_str!("../src/visual_capture.rs");

    assert!(lib.contains("visual_capture::capture_progressive_target"));
    assert!(module.contains("pub async fn capture_progressive_target("));
    assert!(module.contains("/semantic-snapshot/fresh"));
    assert!(module.contains("json::<PageSnapshot>()"));
    assert!(module.contains("resolve_progressive_targets(&snapshot, &reference)"));
    assert!(module.contains("snapshot.viewport"));
    assert!(module.contains("progressive target viewport does not match fresh semantic snapshot"));
    assert!(module.contains(".find(|target| target.kind == level)"));
    assert!(module.contains("requested progressive target level is unavailable"));
    assert!(!module.contains("unwrap_or(ProgressiveTargetKind::Viewport)"));
    assert!(!module.contains("unwrap_or_else(|| RequestedCaptureTarget::Viewport"));
}

#[test]
fn progressive_target_capture_reuses_one_shared_native_transaction_and_binds_live_state() {
    let module = include_str!("../src/visual_capture.rs");

    let start = module
        .find("pub async fn capture_progressive_target(")
        .expect("progressive target capture command must exist");
    let end = module[start..]
        .find("\nasync fn ")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let preflight = command
        .find("preflight_managed_surface(&app, session_id)?")
        .expect("progressive capture must preflight the exact managed surface");
    let gate = command
        .find("session_capture_gate(&state, session_id).await?")
        .expect("progressive capture must use the per-session capture gate");
    let fresh = command
        .find("fresh_semantic_snapshot(session_id).await?")
        .expect("progressive capture must fetch a fresh semantic snapshot inside the gate");
    let resolve = command
        .find("resolve_progressive_targets(&snapshot, &reference)")
        .expect("progressive capture must resolve against that fresh snapshot");
    let acquire = command
        .find("capture_redacted_viewport_after_gate(")
        .expect("progressive capture must reuse the shared audited native transaction");
    let live = command
        .find("validate_progressive_live_state(")
        .expect("progressive capture must reject route/viewport drift after acquisition");
    let crop = command
        .find("apply_capture_target(")
        .expect("progressive capture must crop the resolved target after redaction");
    let persist = command
        .find("persist_and_register")
        .expect("progressive capture must persist only processed target pixels");

    assert!(preflight < gate);
    assert!(gate < fresh);
    assert!(fresh < resolve);
    assert!(resolve < acquire);
    assert!(acquire < live);
    assert!(live < crop);
    assert!(crop < persist);
    assert_eq!(
        command.matches("capture_redacted_viewport_after_gate(").count(),
        1,
        "progressive target capture must acquire native pixels once"
    );
    assert!(!command.contains("capture_managed_surface("));
}

#[test]
fn progressive_target_capture_keeps_platform_adapters_viewport_only_and_preserves_provenance() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("target: CaptureTarget::Viewport"));
    assert!(module.contains("ProgressiveTargetCaptureReceipt"));
    assert!(module.contains("ProgressiveTargetKind"));
    assert!(module.contains("ProgressiveTargetProvenance"));
    assert!(module.contains("confidence_milli"));
    assert!(module.contains("snapshot_version"));
    assert!(module.contains("snapshot_route"));
    assert!(module.contains("RequestedCaptureTarget::Region(resolved.rect.clone())"));
    assert!(module.contains("ProgressiveTargetKind::Viewport => RequestedCaptureTarget::Viewport"));
}

#[test]
fn progressive_target_capture_privacy_order_remains_restore_redact_then_crop() {
    let module = include_str!("../src/visual_capture.rs");

    let helper_start = module
        .find("async fn capture_redacted_viewport_after_gate(")
        .expect("shared redacted viewport helper must exist");
    let helper_end = module[helper_start..]
        .find("\nasync fn ")
        .map(|offset| helper_start + offset)
        .unwrap_or(module.len());
    let helper = &module[helper_start..helper_end];

    let restore = helper
        .find("restore_visual_state(session_id, &freeze.token).await")
        .expect("shared transaction must restore the exact visual lease");
    let redact = helper
        .find("redact_private_pixels(frame, &freeze)?")
        .expect("shared transaction must redact private pixels");
    assert!(restore < redact);

    let command_start = module
        .find("pub async fn capture_progressive_target(")
        .expect("progressive target command must exist");
    let command_end = module[command_start..]
        .find("\nasync fn ")
        .map(|offset| command_start + offset)
        .unwrap_or(module.len());
    let command = &module[command_start..command_end];
    assert!(command.contains("apply_capture_target("));
    assert!(command.contains("persist_and_register"));
}

#[test]
fn progressive_target_capture_fails_closed_on_snapshot_and_live_drift() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("progressive target viewport does not match fresh semantic snapshot"));
    assert!(module.contains(
        "progressive target live viewport drifted from fresh semantic snapshot; pixels discarded"
    ));
    assert!(module.contains(
        "progressive target live route drifted from fresh semantic snapshot; pixels discarded"
    ));
    assert!(module.contains("progressive_route_signature(&frame.route)?"));
    assert!(module.contains("progressive_route_signature(&snapshot.route)?"));
    assert!(module.contains("url.port_or_known_default()"));
    assert!(module.contains("\"[REDACTED]\".to_string()"));
}

#[test]
fn progressive_target_capture_never_widens_missing_component_or_section_requests() {
    let module = include_str!("../src/visual_capture.rs");
    let start = module
        .find("pub async fn capture_progressive_target(")
        .expect("progressive target command must exist");
    let end = module[start..]
        .find("\nasync fn ")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let select = command
        .find(".find(|target| target.kind == level)")
        .expect("requested target level must be selected exactly");
    let reject = command
        .find("requested progressive target level is unavailable")
        .expect("missing requested level must fail closed");
    let map = command
        .find("let target = match level")
        .expect("only an already-resolved exact level may map to pixels");

    assert!(select < reject);
    assert!(reject < map);
    assert!(!command.contains("ProgressiveTargetKind::Component => RequestedCaptureTarget::Viewport"));
    assert!(!command.contains("ProgressiveTargetKind::Section => RequestedCaptureTarget::Viewport"));
}
