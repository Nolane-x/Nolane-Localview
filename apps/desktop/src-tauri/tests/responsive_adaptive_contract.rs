fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn live_adaptive_probe_reuses_exact_preview_settle_and_fresh_snapshot_authority() {
    let source = include_str!("../src/visual_capture.rs");
    let adaptive = between(
        source,
        "impl LiveResponsiveLayoutProbe<'_> {",
        "async fn wait_for_responsive_size_convergence(",
    );

    for required in [
        "DEFAULT_ADAPTIVE_PROBE_CAP",
        "bounded_adaptive_sweep",
        "discover_breakpoint",
        "wait_for_capture_settle",
        "fresh_semantic_snapshot",
        "validate_responsive_preview_authority",
        "responsive_session_drift",
        "responsive_route_drift",
        "responsive_probe_cap_exceeded",
        "semantic_state_drift",
        "semantic_state_fingerprint_incomplete",
        "adaptive_height_drift",
        "resolve_observed_transition",
        "analyze_responsive_series",
    ] {
        assert!(adaptive.contains(required), "adaptive runtime missing {required}");
    }

    assert!(
        !adaptive.contains("capture_managed_surface"),
        "adaptive widths must not become a screenshot farm"
    );
    assert!(
        !adaptive.contains("artifacts.put("),
        "adaptive probes must retain no per-width visual artifacts"
    );
    assert!(
        !adaptive.to_ascii_lowercase().contains("chromium"),
        "adaptive execution must not create a Chromium path"
    );
}

#[test]
fn live_probe_cache_eliminates_duplicate_width_execution() {
    let source = include_str!("../src/visual_capture.rs");
    let probe = between(
        source,
        "impl LiveResponsiveLayoutProbe<'_> {",
        "impl LayoutProbe for LiveResponsiveLayoutProbe<'_>",
    );

    let cache_read = probe
        .find("state.probes.get(&width)")
        .expect("same-width cache read must exist");
    let cap_check = probe
        .find("state.attempted_widths.len() >= self.probe_cap")
        .expect("probe cap must exist");
    let resize = probe
        .find(".set_size(tauri::LogicalSize::new")
        .expect("live resize must exist");
    assert!(cache_read < cap_check && cap_check < resize);
}

#[test]
fn adaptive_failures_still_flow_through_outer_restore() {
    let source = include_str!("../src/visual_capture.rs");
    let tx = between(
        source,
        "pub async fn capture_responsive_sweep(",
        "async fn wait_for_responsive_size_convergence(",
    );

    let adaptive = tx.find("run_live_adaptive_responsive").unwrap();
    let timeout = tx
        .find("responsive_transaction_timeout")
        .expect("bounded transaction timeout");
    let restore = tx.find("restore_responsive_preview").unwrap();
    assert!(adaptive < timeout);
    assert!(timeout < restore);

    for failure in [
        "responsive_resize_failed",
        "responsive_settle_failed",
        "responsive_route_drift",
        "responsive_session_drift",
        "responsive_evidence_capture_failed",
        "responsive_probe_cap_exceeded",
    ] {
        assert!(source.contains(failure), "missing fail-closed class {failure}");
    }

    for canonical_failure in [
        "responsive_native_capture_failed",
        "responsive_restore_failed",
        "responsive_redaction_failed",
    ] {
        assert!(
            source.contains(canonical_failure),
            "canonical capture authority lost {canonical_failure}"
        );
    }
}

#[test]
fn observed_transition_is_not_claimed_as_css_media_query_breakpoint() {
    let responsive = include_str!("../../../../crates/responsive/src/lib.rs");
    let transition = between(
        responsive,
        "pub enum ObservedTransitionResolution",
        "pub fn bounded_adaptive_sweep",
    );
    assert!(transition.contains("lower_width"));
    assert!(transition.contains("upper_width"));

    let resolver = between(
        responsive,
        "pub fn resolve_observed_transition(",
        "#[allow(async_fn_in_trait)]",
    );
    assert!(resolver.contains("observed_responsive_transition"));
    assert!(resolver.contains("non_monotonic_detector"));
    assert!(resolver.contains("same_width_detector_instability"));
    assert!(!resolver.contains("media_query"));
    assert!(!resolver.contains("css_breakpoint"));
    assert!(!resolver.contains("exact_breakpoint"));
}

#[test]
fn deeper_issue_model_is_deterministic_bounded_and_non_aesthetic() {
    let responsive = include_str!("../../../../crates/responsive/src/lib.rs");
    for issue in [
        "HorizontalOverflow",
        "Clipping",
        "UnexpectedDisappearance",
        "ControlCollision",
        "DramaticLayoutJump",
        "TextOrControlOutsideViewport",
        "BreakpointLocalRegression",
        "NearbyWidthInstability",
    ] {
        assert!(responsive.contains(issue), "responsive issue kind missing {issue}");
    }

    for evidence_field in [
        "session:",
        "route:",
        "viewport:",
        "refs:",
        "detector:",
        "evidence:",
        "confidence_milli:",
        "class:",
        "before_width:",
        "after_width:",
    ] {
        assert!(responsive.contains(evidence_field), "issue evidence missing {evidence_field}");
    }

    let lower = responsive.to_ascii_lowercase();
    assert!(!lower.contains("ui is ugly"));
    assert!(!lower.contains("aesthetic score"));
}

#[test]
fn truncated_semantic_projection_cannot_become_a_false_pass() {
    let responsive = include_str!("../../../../crates/responsive/src/lib.rs");
    let evaluator = between(
        responsive,
        "pub fn evaluate_responsive_observation(",
        "pub fn analyze_responsive_series(",
    );
    assert!(evaluator.contains("!observation.complete"));
    assert!(evaluator.contains("ResponsiveDetectorState::Inconclusive"));
}
