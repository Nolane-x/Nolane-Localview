#[test]
fn desktop_native_capture_is_managed_bounded_and_metadata_only() {
    let lib = include_str!("../src/lib.rs");
    let module = include_str!("../src/visual_capture.rs");

    assert!(lib.contains("#![forbid(unsafe_code)]"));
    assert!(lib.contains("capture_viewport"));
    assert!(lib.contains("capture_region"));
    assert!(lib.contains("VisualCaptureState"));
    assert!(module.contains("ArtifactStore"));
    assert!(module.contains("256 * 1024 * 1024"));
    assert!(module.contains("with_webview"));
    assert!(module.contains("fn evidence_suffix"));
    assert!(module.contains("Self::Viewport => \"visual\""));
    assert!(module.contains("Self::Region(_) => \"visual-region\""));
    assert!(module.contains("/evidence/{}"));
    assert!(module.contains("target: CaptureTarget::Viewport"));
    assert!(module.contains("window.url()"));
    assert!(module.contains("bridge_surface_label_allowed"));
    assert!(module.contains("workspace_navigation_allowed"));
    assert!(module.contains("tokio::time::timeout"));
    assert!(module.contains("Duration::from_secs(3)"));
    assert!(module.contains("visual/png"));
    assert!(module.contains("crop_png_css_rect"));
    assert!(!module.contains("base64"));
    assert!(!module.contains("html2canvas"));
    assert!(!module.contains("canvas.toDataURL"));
}

#[test]
fn region_capture_restores_then_redacts_then_crops_before_persistence() {
    let module = include_str!("../src/visual_capture.rs");

    let redacted_helper = module
        .split("async fn capture_redacted_viewport_after_gate(")
        .nth(1)
        .expect("redacted viewport helper must exist")
        .split("async fn capture_target(")
        .next()
        .expect("redacted viewport helper must be bounded");
    let restore = redacted_helper
        .find("let restore_result = restore_visual_state")
        .expect("capture transaction must restore visual state");
    let redact = redacted_helper
        .find("let frame = redact_private_pixels")
        .expect("capture transaction must redact private pixels");
    assert!(
        restore < redact,
        "visual restoration must happen before private pixel processing"
    );

    let target = module
        .split("async fn capture_target(")
        .nth(1)
        .expect("target capture transaction must exist")
        .split("async fn compatible_changed_baseline(")
        .next()
        .expect("target capture transaction must be bounded");
    let redacted_capture = target
        .find("capture_redacted_viewport_after_gate")
        .expect("target capture must use the restored/redacted viewport helper");
    let crop = target
        .find("let frame = apply_capture_target")
        .expect("target capture must apply the requested target");
    let persist = target
        .find("persist_and_register")
        .expect("target capture must persist/register evidence");

    assert!(
        redacted_capture < crop,
        "private-redacted capture must happen before region cropping"
    );
    assert!(crop < persist, "target processing must happen before persistence");
}
