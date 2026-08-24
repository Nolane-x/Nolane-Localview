#[test]
fn unsafe_boundary_stays_out_of_safe_product_crates() {
    let desktop = include_str!("../../../apps/desktop/src-tauri/src/lib.rs");
    let capture = include_str!("../../capture/src/lib.rs");
    let artifacts = include_str!("../../artifacts/src/lib.rs");
    let evidence = include_str!("../../evidence/src/lib.rs");
    let control = include_str!("../../control/src/lib.rs");
    let common = include_str!("../src/lib.rs");

    assert!(desktop.contains("#![forbid(unsafe_code)]"));
    assert!(capture.contains("#![forbid(unsafe_code)]"));
    assert!(artifacts.contains("#![forbid(unsafe_code)]"));
    assert!(evidence.contains("#![forbid(unsafe_code)]"));
    assert!(control.contains("#![forbid(unsafe_code)]"));
    assert!(common.contains("#![deny(unsafe_op_in_unsafe_fn)]"));
    assert!(common.contains("pub fn capture_webview"));
    assert!(!common.contains("*mut "));
    assert!(!common.contains("*const "));
}

#[test]
fn any_explicit_platform_unsafe_is_documented() {
    let platform_sources = [
        include_str!("../src/platform/windows.rs"),
        include_str!("../src/platform/macos.rs"),
        include_str!("../src/platform/linux.rs"),
    ];

    for source in platform_sources {
        if source.contains("unsafe {") {
            assert!(source.contains("// SAFETY:"));
        }
        assert!(!source.contains("html2canvas"));
        assert!(!source.contains("canvas.toDataURL"));
    }
}
