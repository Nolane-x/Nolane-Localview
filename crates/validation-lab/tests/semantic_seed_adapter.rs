use std::collections::BTreeSet;

use localview_validation_lab::{
    LabSeed, LabSeedIdentity, ResultEvidence, SemanticSeedLabRecord, adapt_semantic_seed_outcome,
};
use serde_json::json;

fn seed(expected: &str, forbidden: &[&str], comparison_mode: &str) -> LabSeed {
    LabSeed {
        identity: LabSeedIdentity {
            seed_id: "LV-S-L1-001".into(),
            prediction_revision: "pred-r1".into(),
            oracle_revision: "oracle-r1".into(),
        },
        family: "deterministic-semantic".into(),
        spec_surface_refs: BTreeSet::from([841, 843]),
        input_fixture: json!({"generation": 7, "incarnation": "i1"}),
        expected_semantic_outcome: expected.into(),
        forbidden_outcomes: forbidden.iter().map(|value| (*value).to_owned()).collect(),
        comparison_mode: comparison_mode.into(),
        risk_if_missed: "high".into(),
    }
}

#[test]
fn exact_expected_outcome_is_a_seed_pass_without_inventing_metric_authority() {
    let record = adapt_semantic_seed_outcome(
        &seed("STALE", &["CURRENT"], "exact"),
        "STALE",
        ["trace:b", "trace:a", "trace:a"],
        "compare-r7",
        21,
    )
    .unwrap();

    let SemanticSeedLabRecord {
        observation,
        result_evidence,
        forbidden_outcome_observed,
    } = record;

    assert_eq!(result_evidence, ResultEvidence::PreregisteredSeedPass);
    assert!(!forbidden_outcome_observed);
    assert_eq!(observation.observation_id, "semantic-seed:LV-S-L1-001");
    assert_eq!(observation.seed_id.as_deref(), Some("LV-S-L1-001"));
    assert_eq!(observation.expected_outcome, "STALE");
    assert_eq!(observation.observed_outcome, "STALE");
    assert!(observation.eligible_metrics.is_empty());
    assert!(observation.failure_flags.is_empty());
    assert_eq!(
        observation.evidence_refs,
        BTreeSet::from(["trace:a".to_owned(), "trace:b".to_owned()])
    );
    assert!(!observation.provider_backed);
    assert_eq!(observation.comparison_profile_revision, "compare-r7");
    assert_eq!(observation.logical_sequence, 21);
}

#[test]
fn exact_mismatch_is_a_counterexample_and_preserves_forbidden_membership() {
    let record = adapt_semantic_seed_outcome(
        &seed("STALE", &["CURRENT", "UNKNOWN"], "exact"),
        "CURRENT",
        ["trace:mismatch"],
        "compare-r7",
        22,
    )
    .unwrap();

    assert_eq!(record.result_evidence, ResultEvidence::CounterexampleFound);
    assert!(record.forbidden_outcome_observed);
    assert_eq!(record.observation.expected_outcome, "STALE");
    assert_eq!(record.observation.observed_outcome, "CURRENT");
    assert!(record.observation.eligible_metrics.is_empty());
    assert!(record.observation.failure_flags.is_empty());
}

#[test]
fn unexpected_non_forbidden_exact_mismatch_is_still_a_counterexample() {
    let record = adapt_semantic_seed_outcome(
        &seed("STALE", &["CURRENT"], "exact"),
        "RECONCILIATION_REQUIRED",
        ["trace:unexpected"],
        "compare-r7",
        23,
    )
    .unwrap();

    assert_eq!(record.result_evidence, ResultEvidence::CounterexampleFound);
    assert!(!record.forbidden_outcome_observed);
    assert!(record.observation.failure_flags.is_empty());
}

#[test]
fn non_exact_comparison_mode_is_rejected_instead_of_guessed() {
    let error = adapt_semantic_seed_outcome(
        &seed("STALE", &["CURRENT"], "semantic-equivalence"),
        "STALE",
        ["trace:mode"],
        "compare-r7",
        24,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "unsupported L1 semantic comparison mode: semantic-equivalence"
    );
}

#[test]
fn empty_comparison_profile_is_rejected_before_observation_authority_is_minted() {
    let error = adapt_semantic_seed_outcome(
        &seed("STALE", &["CURRENT"], "exact"),
        "STALE",
        ["trace:empty-profile"],
        "",
        25,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "authority field comparison_profile_revision cannot be empty"
    );
}
