use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

#[test]
fn visual_freeze_lease_is_bounded_owned_and_motion_only() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "freezeVisuals",
        "restoreVisuals",
        "document.getAnimations",
        "VISUAL_FREEZE_LEASE_MS = 8000",
        "data-localview-visual-freeze",
        "animation-play-state: paused !important",
        "transition-duration: 0s !important",
        "transition-delay: 0s !important",
        "caret-color: transparent !important",
        "scroll-behavior: auto !important",
        "requestAnimationFrame",
    ] {
        assert!(script.contains(required), "missing visual freeze contract: {required}");
    }

    assert!(script.contains("lease.token !== token"));
    assert!(script.contains("animation.playState"));
    assert!(script.contains("animation.pause()"));
    assert!(script.contains("animation.play()"));
}

#[test]
fn visual_freeze_never_monkey_patches_time_or_reconstructs_screenshots() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for forbidden in [
        "window.setTimeout =",
        "window.setInterval =",
        "Date.now =",
        "performance.now =",
        "html2canvas",
        "canvas.toDataURL",
        "toDataURL(",
        "getImageData(",
    ] {
        assert!(
            !script.contains(forbidden),
            "visual freeze introduced forbidden path: {forbidden}"
        );
    }
}
