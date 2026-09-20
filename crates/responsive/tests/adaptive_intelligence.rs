use localview_responsive::{
    analyze_responsive_series, bounded_adaptive_sweep, deduplicate_responsive_issues,
    discover_breakpoint, evaluate_responsive_observation, resolve_observed_transition, LayoutProbe,
    ObservedTransitionResolution, ResponsiveDetectorState, ResponsiveIssueClass,
    ResponsiveIssueKind, ResponsiveNodeObservation, ResponsiveObservation, ResponsiveProbeEvaluation,
    ResponsiveProbeSample, ResponsiveRect, Viewport, DEFAULT_ADAPTIVE_PROBE_CAP,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn rect(x: f64, y: f64, width: f64, height: f64) -> ResponsiveRect {
    ResponsiveRect { x, y, width, height }
}

fn node(
    reference: &str,
    parent_reference: Option<&str>,
    rect: ResponsiveRect,
    interactive: bool,
    text_or_control: bool,
) -> ResponsiveNodeObservation {
    ResponsiveNodeObservation {
        reference: reference.to_string(),
        parent_reference: parent_reference.map(str::to_string),
        rect,
        interactive,
        text_or_control,
    }
}

fn observation(width: u32, version: u64, nodes: Vec<ResponsiveNodeObservation>) -> ResponsiveObservation {
    ResponsiveObservation {
        session: "session-a".to_string(),
        route: "http://127.0.0.1:3000/dashboard".to_string(),
        viewport: Viewport { width, height: 800 },
        snapshot_version: version,
        state_fingerprint: 0x1234,
        state_fingerprint_complete: true,
        complete: true,
        nodes,
    }
}

#[test]
fn adaptive_planner_hard_caps_deduplicates_and_preserves_bounds() {
    let anchors = [320, 320, 360, 390, 430, 768, 1024, 1280, 1440, 1440];
    let widths = bounded_adaptive_sweep(320, 1440, &anchors, 6).unwrap();

    assert!(widths.len() <= 6);
    assert_eq!(widths.first(), Some(&320));
    assert_eq!(widths.last(), Some(&1440));
    assert!(widths.windows(2).all(|pair| pair[0] < pair[1]));
    let primitive = localview_responsive::adaptive_sweep(320, 1440, &anchors);
    assert!(
        widths.iter().all(|width| primitive.contains(width)),
        "bounded planner must select only candidates from the existing adaptive_sweep primitive"
    );
}

#[test]
fn adaptive_planner_rejects_invalid_width_arithmetic_and_caps() {
    assert!(bounded_adaptive_sweep(0, 1440, &[], 6).is_err());
    assert!(bounded_adaptive_sweep(1440, 320, &[], 6).is_err());
    assert!(bounded_adaptive_sweep(320, 1440, &[], 1).is_err());
    assert!(bounded_adaptive_sweep(
        320,
        1440,
        &[],
        DEFAULT_ADAPTIVE_PROBE_CAP + 1
    )
    .is_err());
}

#[test]
fn observed_transition_requires_one_monotonic_bracket() {
    let resolved = resolve_observed_transition(
        &[
            ResponsiveProbeSample { width: 700, state: ResponsiveDetectorState::Fail },
            ResponsiveProbeSample { width: 716, state: ResponsiveDetectorState::Fail },
            ResponsiveProbeSample { width: 728, state: ResponsiveDetectorState::Pass },
        ],
        16,
        "responsive_geometry_v1",
    );

    match resolved {
        ObservedTransitionResolution::Resolved {
            claim,
            lower_width,
            upper_width,
            lower_state,
            upper_state,
            tolerance_px,
            ..
        } => {
            assert_eq!(claim, "observed_responsive_transition");
            assert_eq!((lower_width, upper_width), (716, 728));
            assert_eq!(lower_state, ResponsiveDetectorState::Fail);
            assert_eq!(upper_state, ResponsiveDetectorState::Pass);
            assert!(upper_width - lower_width <= tolerance_px);
            assert_ne!(lower_width, upper_width, "must remain a bracket, not an exact CSS breakpoint");
        }
        other => panic!("expected resolved observed transition, got {other:?}"),
    }
}

#[test]
fn no_transition_is_explicit() {
    let result = resolve_observed_transition(
        &[
            ResponsiveProbeSample { width: 320, state: ResponsiveDetectorState::Pass },
            ResponsiveProbeSample { width: 768, state: ResponsiveDetectorState::Pass },
            ResponsiveProbeSample { width: 1440, state: ResponsiveDetectorState::Pass },
        ],
        16,
        "responsive_geometry_v1",
    );
    assert!(matches!(result, ObservedTransitionResolution::NoTransition { .. }));
}

#[test]
fn non_monotonic_detector_is_inconclusive() {
    let result = resolve_observed_transition(
        &[
            ResponsiveProbeSample { width: 320, state: ResponsiveDetectorState::Fail },
            ResponsiveProbeSample { width: 600, state: ResponsiveDetectorState::Pass },
            ResponsiveProbeSample { width: 900, state: ResponsiveDetectorState::Fail },
        ],
        16,
        "responsive_geometry_v1",
    );
    assert!(matches!(
        result,
        ObservedTransitionResolution::Inconclusive { ref reason, .. }
            if reason == "non_monotonic_detector"
    ));
}

#[test]
fn conflicting_same_width_evidence_is_inconclusive_not_deduplicated_away() {
    let result = resolve_observed_transition(
        &[
            ResponsiveProbeSample { width: 728, state: ResponsiveDetectorState::Pass },
            ResponsiveProbeSample { width: 728, state: ResponsiveDetectorState::Fail },
        ],
        16,
        "responsive_geometry_v1",
    );
    assert!(matches!(
        result,
        ObservedTransitionResolution::Inconclusive { ref reason, .. }
            if reason == "same_width_detector_instability"
    ));
}

struct CountingMonotonicProbe {
    count: AtomicUsize,
}

impl LayoutProbe for CountingMonotonicProbe {
    async fn fails_at(&self, width: u32) -> bool {
        self.count.fetch_add(1, Ordering::Relaxed);
        width < 728
    }
}

#[tokio::test]
async fn binary_discovery_is_bounded_for_the_supported_width_domain() {
    let probe = CountingMonotonicProbe { count: AtomicUsize::new(0) };
    let result = discover_breakpoint(&probe, 1440, 320, 16).await;
    assert!(result.is_some());
    assert!(
        probe.count.load(Ordering::Relaxed) <= DEFAULT_ADAPTIVE_PROBE_CAP,
        "binary primitive alone must fit the live adaptive hard cap"
    );
}

#[test]
fn deterministic_issue_model_covers_overflow_clipping_collision_disappearance_and_jump() {
    let previous = observation(
        390,
        10,
        vec![
            node("@root", None, rect(0.0, 0.0, 390.0, 800.0), false, false),
            node("@gone", Some("@root"), rect(10.0, 10.0, 80.0, 30.0), true, true),
            node("@move", Some("@root"), rect(10.0, 100.0, 80.0, 30.0), true, true),
        ],
    );
    let current = observation(
        320,
        11,
        vec![
            node("@root", None, rect(0.0, 0.0, 320.0, 800.0), false, false),
            node("@parent", Some("@root"), rect(0.0, 0.0, 100.0, 100.0), false, false),
            node("@clip", Some("@parent"), rect(90.0, 10.0, 30.0, 20.0), false, true),
            node("@overflow", Some("@root"), rect(300.0, 200.0, 50.0, 25.0), false, true),
            node("@c1", Some("@root"), rect(20.0, 300.0, 100.0, 40.0), true, true),
            node("@c2", Some("@root"), rect(60.0, 305.0, 100.0, 40.0), true, true),
            node("@move", Some("@root"), rect(230.0, 650.0, 40.0, 20.0), true, true),
        ],
    );

    let previous_evaluation = evaluate_responsive_observation(None, &previous).unwrap();
    let evaluation = evaluate_responsive_observation(None, &current).unwrap();
    assert_eq!(evaluation.state, ResponsiveDetectorState::Fail);

    let mut issues = evaluation.issues.clone();
    issues.extend(
        analyze_responsive_series(
            &[previous.clone(), current.clone()],
            &[previous_evaluation, evaluation.clone()],
        )
        .unwrap(),
    );
    let kinds = issues.iter().map(|issue| issue.kind).collect::<Vec<_>>();
    for required in [
        ResponsiveIssueKind::HorizontalOverflow,
        ResponsiveIssueKind::Clipping,
        ResponsiveIssueKind::TextOrControlOutsideViewport,
        ResponsiveIssueKind::ControlCollision,
        ResponsiveIssueKind::UnexpectedDisappearance,
        ResponsiveIssueKind::DramaticLayoutJump,
    ] {
        assert!(kinds.contains(&required), "missing issue {required:?}");
    }
    assert!(issues.iter().all(|issue| {
        issue
            .evidence
            .iter()
            .any(|entry| entry.starts_with("snapshot_version="))
    }));
    let clipping = issues
        .iter()
        .find(|issue| issue.kind == ResponsiveIssueKind::Clipping)
        .unwrap();
    assert_eq!(clipping.class, ResponsiveIssueClass::Suspected);
}

#[test]
fn breakpoint_local_regression_and_nearby_instability_are_explicit() {
    let observations = vec![
        observation(320, 1, vec![node("@root", None, rect(0.0, 0.0, 320.0, 800.0), false, false)]),
        observation(360, 2, vec![node("@root", None, rect(0.0, 0.0, 360.0, 800.0), false, false)]),
        observation(390, 3, vec![node("@root", None, rect(0.0, 0.0, 390.0, 800.0), false, false)]),
    ];
    let evaluations = vec![
        ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Fail, issues: vec![] },
        ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Pass, issues: vec![] },
        ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Fail, issues: vec![] },
    ];

    let issues = analyze_responsive_series(&observations, &evaluations).unwrap();
    assert!(issues.iter().any(|issue| issue.kind == ResponsiveIssueKind::BreakpointLocalRegression));
    assert!(issues.iter().any(|issue| issue.kind == ResponsiveIssueKind::NearbyWidthInstability));
}

#[test]
fn responsive_issue_dedup_is_bounded_and_stable() {
    let current = observation(
        320,
        4,
        vec![node("@overflow", None, rect(300.0, 10.0, 50.0, 20.0), false, true)],
    );
    let evaluation = evaluate_responsive_observation(None, &current).unwrap();
    let issue = evaluation
        .issues
        .iter()
        .find(|issue| issue.kind == ResponsiveIssueKind::HorizontalOverflow)
        .unwrap()
        .clone();

    let deduped = deduplicate_responsive_issues(vec![issue.clone(), issue]);
    assert_eq!(deduped.len(), 1);
}


#[test]
fn truncated_responsive_projection_is_inconclusive_not_pass() {
    let mut current = observation(
        640,
        20,
        vec![node("@root", None, rect(0.0, 0.0, 640.0, 800.0), false, false)],
    );
    current.complete = false;

    let evaluation = evaluate_responsive_observation(None, &current).unwrap();
    assert_eq!(evaluation.state, ResponsiveDetectorState::Inconclusive);
}

#[test]
fn state_drift_suppresses_cross_width_regression_claims() {
    let left = observation(
        320,
        30,
        vec![node("@root", None, rect(0.0, 0.0, 320.0, 800.0), false, false)],
    );
    let mut middle = observation(
        360,
        31,
        vec![node("@root", None, rect(0.0, 0.0, 360.0, 800.0), false, false)],
    );
    let right = observation(
        390,
        32,
        vec![node("@root", None, rect(0.0, 0.0, 390.0, 800.0), false, false)],
    );
    middle.state_fingerprint = 0x9999;

    let issues = analyze_responsive_series(
        &[left, middle, right],
        &[
            ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Fail, issues: vec![] },
            ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Pass, issues: vec![] },
            ResponsiveProbeEvaluation { state: ResponsiveDetectorState::Fail, issues: vec![] },
        ],
    )
    .unwrap();

    assert!(!issues.iter().any(|issue| {
        matches!(
            issue.kind,
            ResponsiveIssueKind::BreakpointLocalRegression
                | ResponsiveIssueKind::NearbyWidthInstability
        )
    }));
}
