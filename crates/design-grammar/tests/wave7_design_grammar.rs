use localview_design_grammar::{
    DesignEvidenceSamples, DesignMetricSample, EvidenceProvenance, EvidenceStatus,
    ExtractionPolicy, MAX_GRAMMAR_SAMPLES_PER_METRIC, MAX_REFS_PER_FAMILY, MetricFamilies,
    ProjectDesignGrammar, ResponsiveGrammarSnapshot, ResponsiveVariationState,
    extract_project_grammar, responsive_variation,
};

fn sample(value: f64, reference: &str) -> DesignMetricSample {
    DesignMetricSample {
        value,
        reference: reference.into(),
        provenance: EvidenceProvenance {
            snapshot_seq: Some(7),
            snapshot_version: Some(3),
            route: Some("/".into()),
            viewport_width: Some(1280),
            viewport_height: Some(720),
            evidence_keys: vec!["computed_style".into()],
        },
        status: EvidenceStatus::Observed,
    }
}

#[test]
fn repeated_and_noisy_spacing_extracts_only_supported_families() {
    let grammar = extract_project_grammar(
        &DesignEvidenceSamples {
            spacing: vec![
                sample(8.0, "a"),
                sample(8.2, "b"),
                sample(16.0, "c"),
                sample(16.4, "d"),
                sample(31.0, "noise"),
            ],
            observed_nodes: 5,
            ..Default::default()
        },
        ExtractionPolicy::default(),
    );
    let families = grammar.spacing_families.families();
    assert_eq!(families.len(), 2);
    assert!((families[0].center - 8.1).abs() < 0.01);
    assert_eq!(families[0].sample_count, 2);
    assert_eq!(families[0].status, EvidenceStatus::Inferred);
    assert!(families[0].support_ratio > 0.0);
    assert!(!families[0].refs.is_empty());
    assert!(!families[0].provenance.is_empty());
}

#[test]
fn insufficient_sample_and_unsupported_radius_are_unavailable() {
    let one = extract_project_grammar(
        &DesignEvidenceSamples {
            radius: vec![sample(8.0, "only")],
            ..Default::default()
        },
        ExtractionPolicy::default(),
    );
    assert!(matches!(
        one.radius_families,
        MetricFamilies::Unavailable { ref reason } if reason == "insufficient_samples"
    ));

    let none = extract_project_grammar(
        &DesignEvidenceSamples::default(),
        ExtractionPolicy::default(),
    );
    assert!(matches!(
        none.radius_families,
        MetricFamilies::Unavailable { ref reason } if reason == "no_live_evidence"
    ));
}

#[test]
fn typography_and_control_height_families_are_extracted() {
    let grammar = extract_project_grammar(
        &DesignEvidenceSamples {
            font_sizes: vec![
                sample(14.0, "a"),
                sample(14.0, "b"),
                sample(20.0, "c"),
                sample(20.0, "d"),
            ],
            font_weights: vec![
                sample(400.0, "a"),
                sample(400.0, "b"),
                sample(700.0, "c"),
                sample(700.0, "d"),
            ],
            line_heights: vec![sample(20.0, "a"), sample(20.0, "b")],
            control_heights: vec![sample(40.0, "save"), sample(40.5, "cancel")],
            ..Default::default()
        },
        ExtractionPolicy::default(),
    );
    assert_eq!(grammar.type_size_families.families().len(), 2);
    assert_eq!(grammar.font_weight_families.families().len(), 2);
    assert_eq!(grammar.line_height_families.families().len(), 1);
    assert_eq!(grammar.control_height_families.families().len(), 1);
}

#[test]
fn responsive_evidence_variation_remains_viewport_scoped() {
    let grammar_for = |size: f64| {
        extract_project_grammar(
            &DesignEvidenceSamples {
                font_sizes: vec![sample(size, "a"), sample(size, "b")],
                ..Default::default()
            },
            ExtractionPolicy::default(),
        )
    };
    let variation = responsive_variation(&[
        ResponsiveGrammarSnapshot {
            viewport_width: 390,
            grammar: grammar_for(14.0),
        },
        ResponsiveGrammarSnapshot {
            viewport_width: 1440,
            grammar: grammar_for(16.0),
        },
    ]);
    let type_size = variation
        .iter()
        .find(|entry| entry.metric == "type_size")
        .unwrap();
    assert_eq!(type_size.state, ResponsiveVariationState::Varies);
    assert_eq!(type_size.observed_widths, vec![390, 1440]);
}

#[test]
fn per_metric_samples_and_family_refs_are_bounded() {
    let grammar = extract_project_grammar(
        &DesignEvidenceSamples {
            spacing: (0..3_000)
                .map(|index| sample(8.0, &format!("r{index}")))
                .collect(),
            ..Default::default()
        },
        ExtractionPolicy::default(),
    );
    assert_eq!(
        grammar.spacing_families.sample_count(),
        MAX_GRAMMAR_SAMPLES_PER_METRIC
    );
    assert!(grammar.spacing_families.families()[0].refs.len() <= MAX_REFS_PER_FAMILY);
}

#[test]
fn default_grammar_never_claims_an_official_token() {
    let serialized = serde_json::to_string(&ProjectDesignGrammar::default()).unwrap();
    assert!(!serialized.contains("official_token"));
}
