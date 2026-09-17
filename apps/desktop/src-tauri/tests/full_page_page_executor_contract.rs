#[test]
fn instrumentation_owns_bounded_full_page_freeze_scroll_and_probe_authority() {
    let instrumentation = include_str!("../../../../crates/instrumentation/src/lib.rs");

    assert!(instrumentation.contains("const VIEWPORT_VISUAL_FREEZE_LEASE_MS = 8000;"));
    assert!(instrumentation.contains("const FULL_PAGE_VISUAL_FREEZE_LEASE_MS = 30000;"));
    assert!(instrumentation.contains("const MAX_POSITIONAL_SCAN_ELEMENTS = 4096;"));
    assert!(instrumentation.contains(
        "const freezeVisuals = async (token, leaseMs = VIEWPORT_VISUAL_FREEZE_LEASE_MS) =>"
    ));
    assert!(instrumentation.contains("leaseMs !== VIEWPORT_VISUAL_FREEZE_LEASE_MS"));
    assert!(instrumentation.contains("leaseMs !== FULL_PAGE_VISUAL_FREEZE_LEASE_MS"));
    assert!(instrumentation.contains("originalScrollX"));
    assert!(instrumentation.contains("originalScrollY"));
    assert!(instrumentation.contains("documentGeometry"));

    let scroll_start = instrumentation
        .find("const captureScrollTo = async (token, y) =>")
        .expect("token-bound absolute capture scroll must exist");
    let probe_start = instrumentation[scroll_start..]
        .find("const captureTileProbe = async (token) =>")
        .map(|offset| scroll_start + offset)
        .expect("token-bound tile probe must follow capture scroll");
    let scroll = &instrumentation[scroll_start..probe_start];
    assert!(scroll.contains("visualFreezeLease"));
    assert!(scroll.contains("lease.token !== token"));
    assert!(scroll.contains("window.scrollTo({"));
    assert!(scroll.contains("left: lease.originalScrollX"));
    assert!(scroll.contains("behavior: 'auto'"));
    assert!(scroll.matches("requestAnimationFrame").count() >= 2);
    assert!(!scroll.contains("window.scrollBy"));

    let probe_end = instrumentation[probe_start..]
        .find("window.__LOCALVIEW__ = Object.freeze({")
        .map(|offset| probe_start + offset)
        .expect("tile probe must end before LocalView export");
    let probe = &instrumentation[probe_start..probe_end];
    assert!(probe.contains("visualFreezeLease"));
    assert!(probe.contains("MAX_POSITIONAL_SCAN_ELEMENTS"));
    assert!(probe.contains("full_page_positional_scan_budget_exceeded"));
    assert!(probe.contains("visible_fixed_or_sticky"));
    assert!(probe.contains("positional_elements_scanned"));
    assert!(!probe.contains("innerText"));
    assert!(!probe.contains("textContent"));
    assert!(!probe.contains("mask_selectors"));
}

#[test]
fn managed_webview_executor_keeps_public_scroll_relative_and_full_page_scroll_private() {
    let desktop = include_str!("../src/lib.rs");

    let public_scroll = desktop
        .find("case 'scroll':")
        .expect("existing public relative scroll case must remain");
    let public_scroll_end = desktop[public_scroll..]
        .find("case 'focus':")
        .map(|offset| public_scroll + offset)
        .expect("public scroll case must end before focus");
    assert!(desktop[public_scroll..public_scroll_end].contains("window.scrollBy"));

    assert!(desktop.contains("case 'capture_scroll_to':"));
    assert!(desktop.contains("captureScrollTo?.(action.token, action.y)"));
    assert!(desktop.contains("case 'capture_tile_probe':"));
    assert!(desktop.contains("captureTileProbe?.(action.token)"));
    assert!(desktop.contains("privateMaskGeometry(queued.private_capture?.mask_selectors || [])"));
    assert!(desktop.contains("queued.private_capture?.visual_freeze_lease_ms"));
    assert!(desktop.contains("freezeVisuals?.(queued.id, leaseMs)"));
}
