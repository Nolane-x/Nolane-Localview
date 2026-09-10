use localview_mutation::{MutationOutcome, MutationVerdict};
use localview_validation_lab::{
    LabFailureFlag, LabMetricKind, MutationLabRecord, MutationNotMeasuredReason, ResultEvidence,
    adapt_mutation_outcome,
};

fn outcome(verdict: MutationVerdict, evidence_ids: &[&str]) -> MutationOutcome {
    serde_json::from_value(serde_json::json!({
        "mutation_id": "00000000-0000-4000-8000-000000000042",
        "verdict": verdict,
        "triggered_detectors": [],
        "evidence_ids": evidence_ids,
    }))
    .expect("fixture is a valid mutation outcome")
}

#[test]
fn survived_mutation_becomes_measured_msr_failure_without_inventing_a_new_verdict() {
    let record = adapt_mutation_outcome(
        &outcome(MutationVerdict::Survived, &["ev-b", "ev-a", "ev-a"]),
        Some("seed-42"),
        "cmp-v7",
        43,
    )
    .expect("supported mutation verdict adapts");

    let MutationLabRecord::Measured {
        observation,
        result_evidence,
    } = record
    else {
        panic!("survived mutation must be measured")
    };

    assert_eq!(result_evidence, ResultEvidence::MutantSurvived);
    assert_eq!(observation.seed_id.as_deref(), Some("seed-42"));
    assert_eq!(observation.expected_outcome, "mutant_killed");
    assert_eq!(observation.observed_outcome, "mutant_survived");
    assert_eq!(observation.eligible_metrics, [LabMetricKind::Msr].into());
    assert_eq!(
        observation.failure_flags,
        [LabFailureFlag::MutationSurvived].into()
    );
    assert_eq!(
        observation.evidence_refs,
        ["ev-a".to_string(), "ev-b".to_string()].into()
    );
    assert!(!observation.provider_backed);
    assert_eq!(observation.comparison_profile_revision, "cmp-v7");
    assert_eq!(observation.logical_sequence, 43);
}

#[test]
fn killed_mutation_is_measured_without_incrementing_msr_failure_numerator() {
    let record = adapt_mutation_outcome(
        &outcome(MutationVerdict::Killed, &["ev-kill"]),
        None,
        "cmp-v7",
        44,
    )
    .expect("supported mutation verdict adapts");

    let MutationLabRecord::Measured {
        observation,
        result_evidence,
    } = record
    else {
        panic!("killed mutation must be measured")
    };

    assert_eq!(result_evidence, ResultEvidence::MutantKilled);
    assert_eq!(observation.eligible_metrics, [LabMetricKind::Msr].into());
    assert!(observation.failure_flags.is_empty());
}

#[test]
fn invalid_and_unsafe_skipped_mutations_are_typed_not_measured_records() {
    for (verdict, expected_reason) in [
        (
            MutationVerdict::Invalid,
            MutationNotMeasuredReason::InvalidMutation,
        ),
        (
            MutationVerdict::SkippedUnsafe,
            MutationNotMeasuredReason::SkippedUnsafe,
        ),
    ] {
        let record = adapt_mutation_outcome(&outcome(verdict, &[]), None, "cmp-v7", 45)
            .expect("non-measured mutation outcome remains representable");
        assert_eq!(
            record,
            MutationLabRecord::NotMeasured {
                mutation_id: "00000000-0000-4000-8000-000000000042".into(),
                reason: expected_reason,
            }
        );
    }
}
