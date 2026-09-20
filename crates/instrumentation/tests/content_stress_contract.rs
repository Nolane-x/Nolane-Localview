use localview_instrumentation::{InstrumentationConfig, bootstrap_script};

#[test]
fn content_stress_runtime_is_bounded_private_and_transactional() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    for required in [
        "CONTENT_STRESS_MAX_NODES = 160",
        "CONTENT_STRESS_MAX_SOURCE_CHARS = 512",
        "CONTENT_STRESS_MAX_STRESSED_CHARS = 768",
        "expanded_130",
        "expanded_180",
        "dense_cjk",
        "rtl_pseudo",
        "beginContentStress",
        "restoreContentStress",
        "takeContentStressCompletions",
        "restore_conflict",
        "[data-localview-private]",
        "[data-private]",
        "[data-sensitive]",
    ] {
        assert!(script.contains(required), "missing content-stress contract: {required}");
    }
}

#[test]
fn content_stress_completion_does_not_transport_original_or_stressed_text() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    let start = script
        .find("const queueContentStressCompletion")
        .expect("content-stress completion queue");
    let end = script[start..]
        .find("const contentStressEligibleParent")
        .map(|offset| start + offset)
        .expect("content-stress completion queue boundary");
    let completion = &script[start..end];

    for forbidden in [
        "textContent",
        "innerHTML",
        "originalText",
        "stressedText",
        "nodeValue",
        "password",
        "value:",
    ] {
        assert!(
            !completion.contains(forbidden),
            "completion transport must not contain {forbidden}"
        );
    }
    for required in [
        "requestToken",
        "route",
        "status",
        "profile",
        "mutatedNodes",
        "restoredNodes",
        "conflictNodes",
    ] {
        assert!(completion.contains(required));
    }
}

#[test]
fn content_stress_skips_sensitive_editable_and_code_surfaces() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    for selector in [
        "input,textarea,select,option,pre,code,kbd,samp",
        "[contenteditable=\"true\"]",
        "[aria-hidden=\"true\"]",
    ] {
        assert!(script.contains(selector), "missing skip selector: {selector}");
    }
}
