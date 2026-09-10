use std::collections::BTreeSet;

use localview_validation_lab::{
    ComparisonMode, LabMetricKind, LabPreregistration, LabRevisionIdentity, LabSeed,
    LabSeedResult, MetricEvent, MetricMeasurementStatus, ResearchResultClass, RiskLevel,
    SilentUnsoundnessVerdict, aggregate_metrics, classify_seed_result,
    preregistration_digest, result_digest, silent_unsoundness_verdict,
};
use serde_json::json;

fn identity() -> LabRevisionIdentity {
    LabRevisionIdentity {
        lab_revision: "lab-r1".into(),
        seed_corpus_revision: "seeds-r1".into(),
        spec_revision_digest: "spec-sha256".into(),
        reference_reducer_revision: "reducer-r1".into(),
        mutation_catalog_revision: "mut-r1".into(),
        comparison_profile_revision: "cmp-r1".into(),
        random_source_profile: "fixed-u64".into(),
        platform_profile: None,
        start_sequence: 41,
    }
}

fn preregistration(expected_distinction: &str) -> LabPreregistration {
    LabPreregistration {
        identity: identity(),
        model_bound: "single-seed".into(),
        random_seed: 7,
        comparison_rule_revision: "exact-r1".into(),
        expected_distinction: expected_distinction.into(),
        seed_ids: vec!["LV-S041".into()],
    }
}

fn measured_zero_target_events() -> Vec<MetricEvent> {
    [
        LabMetricKind::Suar,
        LabMetricKind::Wpdr,
        LabMetricKind::Pilr,
        LabMetricKind::Pdmr,
        LabMetricKind::Uobrr,
    ]
    .into_iter()
    .map(|kind| MetricEvent {
        kind,
        eligible: true,
        violation: false,
    })
    .collect()
}

#[test]
fn research_taxonomy_never_serializes_a_proved_class() {
    assert_eq!(ResearchResultClass::ALL.len(), 11);
    for class in ResearchResultClass::ALL {
        let encoded = serde_json::to_string(&class).expect("taxonomy serializes");
        assert!(!encoded.contains("proved"));
    }
}

#[test]
fn core_metric_catalog_contains_exactly_fourteen_v43_metrics() {
    assert_eq!(LabMetricKind::ALL.len(), 14);
    assert_eq!(LabMetricKind::ALL.iter().copied().collect::<BTreeSet<_>>().len(), 14);
}

#[test]
fn zero_denominator_is_not_measured_and_has_no_numeric_rate() {
    let summaries = aggregate_metrics(&[]);
    let suar = summaries.get(&LabMetricKind::Suar).expect("all metrics are represented");
    assert_eq!(suar.status, MetricMeasurementStatus::NotMeasured);
    assert_eq!(suar.numerator, 0);
    assert_eq!(suar.denominator, 0);
    assert_eq!(suar.rate(), None);
}

#[test]
fn wrong_principal_dispatch_violation_fails_zero_target_gate() {
    let summaries = aggregate_metrics(&[MetricEvent {
        kind: LabMetricKind::Wpdr,
        eligible: true,
        violation: true,
    }]);
    let wpdr = &summaries[&LabMetricKind::Wpdr];
    assert_eq!(wpdr.numerator, 1);
    assert_eq!(wpdr.denominator, 1);
    assert_eq!(wpdr.rate(), Some(1.0));
    assert_eq!(silent_unsoundness_verdict(&summaries), SilentUnsoundnessVerdict::Fail);
}

#[test]
fn unmeasured_zero_target_metric_keeps_verdict_inconclusive() {
    let mut events = measured_zero_target_events();
    events.retain(|event| event.kind != LabMetricKind::Pilr);
    let summaries = aggregate_metrics(&events);
    assert_eq!(silent_unsoundness_verdict(&summaries), SilentUnsoundnessVerdict::Inconclusive);
}

#[test]
fn measured_zero_target_metrics_with_no_violations_pass() {
    let summaries = aggregate_metrics(&measured_zero_target_events());
    assert_eq!(silent_unsoundness_verdict(&summaries), SilentUnsoundnessVerdict::Pass);
}

#[test]
fn preregistration_digest_changes_when_expected_distinction_changes() {
    let first = preregistration_digest(&preregistration("CURRENT must never follow stale incarnation"))
        .expect("first preregistration encodes");
    let second = preregistration_digest(&preregistration("STALE must be emitted"))
        .expect("second preregistration encodes");
    assert_ne!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
}

#[test]
fn passing_seed_requires_exact_preregistration_authority() {
    assert_eq!(
        classify_seed_result(true, None, "expected"),
        ResearchResultClass::ExploratoryObservation
    );
    assert_eq!(
        classify_seed_result(true, Some("wrong"), "expected"),
        ResearchResultClass::ExploratoryObservation
    );
    assert_eq!(
        classify_seed_result(true, Some("expected"), "expected"),
        ResearchResultClass::PreregisteredSeedPass
    );
}

#[test]
fn failing_seed_remains_a_counterexample_even_without_preregistration() {
    assert_eq!(
        classify_seed_result(false, None, "expected"),
        ResearchResultClass::CounterexampleFound
    );
}

#[test]
fn machine_readable_seed_keeps_expected_and_forbidden_outcomes_distinct() {
    let seed = LabSeed {
        seed_id: "LV-S041".into(),
        family: "freshness".into(),
        spec_surface_refs: vec![841, 843, 1055],
        input_fixture: json!({"generation": 7, "incarnation": "I2"}),
        expected_semantic_outcome: "STALE".into(),
        forbidden_outcomes: BTreeSet::from(["CURRENT".into()]),
        comparison_mode: ComparisonMode::Exact,
        risk_if_missed: RiskLevel::High,
        prediction_revision: "pred-r2".into(),
    };
    let encoded = serde_json::to_value(seed).expect("seed serializes");
    assert_eq!(encoded["expected_semantic_outcome"], "STALE");
    assert_eq!(encoded["forbidden_outcomes"][0], "CURRENT");
}

#[test]
fn result_digest_binds_provenance() {
    let base = LabSeedResult {
        seed_id: "LV-S041".into(),
        used_preregistration_digest: Some("pre-digest".into()),
        expected_preregistration_digest: "pre-digest".into(),
        observed_semantic_outcome: "STALE".into(),
        result_class: ResearchResultClass::PreregisteredSeedPass,
        metric_events: measured_zero_target_events(),
        provenance_ids: vec!["evidence-a".into()],
    };
    let mut changed = base.clone();
    changed.provenance_ids = vec!["evidence-b".into()];

    let base_digest = result_digest(&base).expect("base result encodes");
    let changed_digest = result_digest(&changed).expect("changed result encodes");
    assert_ne!(base_digest, changed_digest);
}
