#[test]
fn desktop_native_capture_is_managed_bounded_and_metadata_only() {
    let lib = include_str!("../src/lib.rs");
    let module = include_str!("../src/visual_capture.rs");

    assert!(lib.contains("#![forbid(unsafe_code)]"));
    assert!(lib.contains("capture_viewport"));
    assert!(lib.contains("VisualCaptureState"));
    assert!(module.contains("ArtifactStore"));
    assert!(module.contains("256 * 1024 * 1024"));
    assert!(module.contains("with_webview"));
    assert!(module.contains("/evidence/visual"));
    assert!(module.contains("window.url()"));
    assert!(module.contains("bridge_surface_label_allowed"));
    assert!(module.contains("workspace_navigation_allowed"));
    assert!(module.contains("tokio::time::timeout"));
    assert!(module.contains("Duration::from_secs(3)"));
    assert!(module.contains("visual/png"));
    assert!(!module.contains("base64"));
    assert!(!module.contains("html2canvas"));
    assert!(!module.contains("canvas.toDataURL"));
}
