use localview_instrumentation::{InstrumentationConfig, bootstrap_script};

#[test]
fn generated_bootstrap_keeps_visual_capture_and_point_select_apis_together() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "freezeVisuals",
        "restoreVisuals",
        "captureScrollTo",
        "captureTileProbe",
        "beginPointSelect",
        "probePointSelect",
        "cancelPointSelect",
        "takePointSelectCompletions",
    ] {
        assert!(
            script.contains(required),
            "generated bootstrap lost required API: {required}"
        );
    }
}

#[test]
fn point_select_uses_real_hit_testing_and_exact_cleanup() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    let point_slice = script
        .split("const pointSelectCanonicalRoute")
        .nth(1)
        .expect("point-select implementation");
    let point_slice = point_slice
        .split("const rectOf")
        .next()
        .expect("bounded point-select implementation");

    for required in [
        "document.elementFromPoint",
        "refFor(freshTarget)",
        "pointer-events:none",
        "data-localview-owned",
        "removeEventListener(type, listener, true)",
        "freezeObserver?.disconnect()",
        "state.overlay?.remove()",
        "target_changed",
        "target_unavailable",
        "route_changed",
    ] {
        assert!(
            point_slice.contains(required),
            "missing point-select invariant: {required}"
        );
    }

    for forbidden in [
        "contentDocument",
        "contentWindow",
        "querySelector(",
        "nth-child",
        "innerHTML",
    ] {
        assert!(
            !point_slice.contains(forbidden),
            "point-select must not use forbidden authority/traversal: {forbidden}"
        );
    }
}

#[test]
fn point_select_completion_transport_is_metadata_only() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    let queue = script
        .split("const queuePointSelectCompletion")
        .nth(1)
        .expect("completion queue")
        .split("const cleanupPointSelect")
        .next()
        .expect("bounded completion queue");

    for required in [
        "requestToken",
        "route",
        "status",
        "reference",
        "reason",
    ] {
        assert!(
            queue.contains(required),
            "missing bounded receipt field: {required}"
        );
    }

    for forbidden in [
        "innerHTML",
        "textContent",
        ".value",
        "attributes",
        "props",
        "state",
        "hooks",
        "cookies",
        "storage",
    ] {
        assert!(
            !queue.contains(forbidden),
            "private payload escaped point-select receipt: {forbidden}"
        );
    }
}
