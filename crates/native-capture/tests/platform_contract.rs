fn assert_no_browser_fallback(source: &str) {
    for forbidden in [
        "html2canvas",
        "canvas.toDataURL",
        "playwright",
        "launch_chromium",
        "base64::",
        "STANDARD.encode",
    ] {
        assert!(
            !source.contains(forbidden),
            "native capture backend must not contain fallback token: {forbidden}"
        );
    }
}

#[test]
fn native_backends_do_not_fall_back_to_browser_reconstruction() {
    assert_no_browser_fallback(include_str!("../src/platform/windows.rs"));
    assert_no_browser_fallback(include_str!("../src/platform/macos.rs"));
    assert_no_browser_fallback(include_str!("../src/platform/linux.rs"));
}

#[cfg(windows)]
#[test]
fn windows_backend_uses_webview2_capture_preview_and_common_frame_builder() {
    let source = include_str!("../src/platform/windows.rs");
    assert!(source.contains("CapturePreview"));
    assert!(source.contains("CapturePreviewCompletedHandler"));
    assert!(source.contains("build_frame"));
}

#[cfg(target_os = "macos")]
#[test]
fn macos_backend_uses_wkwebview_snapshot_and_common_frame_builder() {
    let source = include_str!("../src/platform/macos.rs");
    assert!(source.contains("WKSnapshotConfiguration"));
    assert!(source.contains("takeSnapshotWithConfiguration_completionHandler"));
    assert!(source.contains("build_frame"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_backend_uses_webkitgtk_snapshot_and_common_frame_builder() {
    let source = include_str!("../src/platform/linux.rs");
    assert!(source.contains("get_snapshot"));
    assert!(source.contains("SnapshotRegion::Visible"));
    assert!(source.contains("build_frame"));
}
