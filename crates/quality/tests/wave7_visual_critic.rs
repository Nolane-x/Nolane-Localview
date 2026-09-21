use localview_design_grammar::{
    DesignEvidenceSamples, DesignMetricSample, EvidenceProvenance, EvidenceStatus,
    ExtractionPolicy, MetricFamilies, ProjectDesignGrammar, extract_project_grammar,
};
use localview_protocol::Rect;
use localview_quality::{
    CriticEvidenceClass, CriticNode, CriticSourceHint, FeatureState, MAX_CRITIC_FINDINGS,
    MAX_CRITIC_NODES, MAX_FINDING_REFS, SourceHintAuthority, VisualCriticReport,
    analyze_visual_critic, build_design_baseline, diff_design_baselines, overlay_model,
    subjective_finding,
};

fn node(reference: &str, x: f64, width: f64, height: f64, font_size: f64) -> CriticNode {
    CriticNode {
        reference: reference.into(),
        rect: Rect {
            x,
            y: 0.0,
            width,
            height,
        },
        interactive: false,
        font_size: Some(font_size),
        font_weight: Some(400.0),
        line_height: Some(font_size * 1.4),
        contrast: Some(4.5),
        source_hint: None,
    }
}

fn sample(value: f64, reference: &str) -> DesignMetricSample {
    DesignMetricSample {
        value,
        reference: reference.into(),
        provenance: EvidenceProvenance::default(),
        status: EvidenceStatus::Observed,
    }
}

fn grammar() -> ProjectDesignGrammar {
    extract_project_grammar(
        &DesignEvidenceSamples {
            font_sizes: vec![
                sample(16.0, "a"),
                sample(16.0, "b"),
                sample(16.0, "c"),
                sample(24.0, "d"),
                sample(24.0, "e"),
                sample(24.0, "f"),
            ],
            font_weights: vec![
                sample(400.0, "a"),
                sample(400.0, "b"),
                sample(400.0, "c"),
            ],
            control_heights: vec![
                sample(40.0, "a"),
                sample(40.0, "b"),
                sample(40.0, "c"),
            ],
            ..Default::default()
        },
        ExtractionPolicy::default(),
    )
}

#[test]
fn deterministic_drift_only_claims_measured_deviation() {
    let report = analyze_visual_critic(
        &[node("outlier", 0.0, 100.0, 40.0, 19.0)],
        (100.0, 100.0),
        &grammar(),
    );
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "observed_scale_deviation")
        .unwrap();
    assert_eq!(finding.class, CriticEvidenceClass::Deterministic);
    assert!(finding.evidence_summary.contains("differs"));
    assert!(finding.measured_values[0].deviation.is_some());
    assert!(!finding.can_trigger_automatic_fix());
}

#[test]
fn measured_density_can_only_produce_heuristic_too_dense() {
    let nodes = (0..24)
        .map(|index| node(&format!("n{index}"), 0.0, 100.0, 100.0, 16.0))
        .collect::<Vec<_>>();
    let report = analyze_visual_critic(&nodes, (100.0, 100.0), &grammar());
    assert!(matches!(report.density, FeatureState::Available(_)));
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "excessive_density_candidate")
        .unwrap();
    assert_eq!(finding.class, CriticEvidenceClass::Heuristic);
    assert!(!finding.can_fail_ci());
}

#[test]
fn balance_is_heuristic_not_a_bug() {
    let report = analyze_visual_critic(
        &[
            node("left", 0.0, 90.0, 100.0, 16.0),
            node("right", 95.0, 5.0, 5.0, 16.0),
        ],
        (100.0, 100.0),
        &grammar(),
    );
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "visual_mass_imbalance_candidate")
        .unwrap();
    assert_eq!(finding.class, CriticEvidenceClass::Heuristic);
    assert!(!finding.can_fail_ci());
}

#[test]
fn hierarchy_uses_observed_visual_features_not_business_importance() {
    let report = analyze_visual_critic(
        &[
            node("hero", 0.0, 100.0, 60.0, 32.0),
            node("body", 0.0, 100.0, 40.0, 14.0),
        ],
        (100.0, 100.0),
        &grammar(),
    );
    let FeatureState::Available(value) = report.hierarchy else {
        panic!("hierarchy must be available");
    };
    assert_eq!(value.entries[0].reference, "hero");
    assert!(value.salience_spread > 0.0);
}

#[test]
fn subjective_class_never_depends_on_confidence() {
    let high = subjective_finding(
        "high",
        "visually_heavy",
        vec!["hero".into()],
        "Composition feels visually heavy",
        1.0,
    );
    let low = subjective_finding(
        "low",
        "visually_heavy",
        vec!["hero".into()],
        "Composition feels visually heavy",
        0.1,
    );
    assert_eq!(high.class, CriticEvidenceClass::Subjective);
    assert_eq!(low.class, CriticEvidenceClass::Subjective);
    assert!(!high.can_fail_ci());
    assert!(!high.can_trigger_automatic_fix());
}

#[test]
fn source_hint_is_optional_and_stable_refs_survive() {
    let mut target = node("stable:save", 0.0, 100.0, 40.0, 19.0);
    target.source_hint = Some(CriticSourceHint {
        file: "src/button.css".into(),
        line: None,
        column: None,
        authority: SourceHintAuthority::ObservedRuntimeHint,
    });
    let report = analyze_visual_critic(&[target], (100.0, 100.0), &grammar());
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.code == "observed_scale_deviation")
        .unwrap();
    assert_eq!(finding.affected_refs, vec!["stable:save"]);
    assert_eq!(finding.source_hints.len(), 1);
    assert_eq!(finding.source_hints[0].line, None);
}

#[test]
fn overlay_model_preserves_class_and_suppresses_capture() {
    let report = analyze_visual_critic(
        &[node("outlier", 0.0, 100.0, 40.0, 19.0)],
        (100.0, 100.0),
        &grammar(),
    );
    let overlay = overlay_model(&report, Some("missing"));
    assert!(overlay.suppress_during_evidence_capture);
    assert!(
        overlay
            .items
            .iter()
            .all(|item| item.class == CriticEvidenceClass::Deterministic)
    );
}

#[test]
fn baseline_no_change_has_no_real_change() {
    let report = analyze_visual_critic(
        &[node("a", 0.0, 100.0, 40.0, 16.0)],
        (100.0, 100.0),
        &grammar(),
    );
    let baseline = build_design_baseline(grammar(), &report);
    let diff = diff_design_baselines(&baseline, &baseline);
    assert!(diff.added_families.is_empty());
    assert!(diff.removed_families.is_empty());
    assert!(diff.scale_drift.is_empty());
    assert!(diff.distribution_changes.is_empty());
    assert!(diff.hierarchy_regressions.is_empty());
    assert!(diff.confidence_changes.is_empty());
}

#[test]
fn baseline_real_drift_is_reported() {
    let report = analyze_visual_critic(
        &[node("a", 0.0, 100.0, 40.0, 16.0)],
        (100.0, 100.0),
        &grammar(),
    );
    let before = build_design_baseline(grammar(), &report);
    let mut after = before.clone();
    if let MetricFamilies::Available { families, .. } = &mut after.grammar.type_size_families {
        families[0].center += 2.0;
        families[0].minimum += 2.0;
        families[0].maximum += 2.0;
    }
    let diff = diff_design_baselines(&before, &after);
    assert!(
        diff.scale_drift
            .iter()
            .any(|change| change.metric == "type_size")
    );
}

#[test]
fn missing_evidence_is_inconclusive_not_zero() {
    let baseline =
        build_design_baseline(ProjectDesignGrammar::default(), &VisualCriticReport::default());
    let diff = diff_design_baselines(&baseline, &baseline);
    assert!(
        diff.inconclusive
            .iter()
            .any(|reason| reason.contains("evidence"))
    );
}

#[test]
fn critic_input_findings_and_refs_are_bounded() {
    let nodes = (0..700)
        .map(|index| node(&format!("n{index}"), 0.0, 100.0, 100.0, 19.0))
        .collect::<Vec<_>>();
    let report = analyze_visual_critic(&nodes, (100.0, 100.0), &grammar());
    assert_eq!(report.analyzed_nodes, MAX_CRITIC_NODES);
    assert!(report.input_truncated);
    assert!(report.findings.len() <= MAX_CRITIC_FINDINGS);
    assert!(
        report
            .findings
            .iter()
            .all(|finding| finding.affected_refs.len() <= MAX_FINDING_REFS)
    );
}
