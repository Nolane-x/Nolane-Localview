use std::collections::BTreeSet;

use localview_validation_lab::{
    CanonicalDigest, LabMetricKind, RealProviderCaseInput, RealProviderCaseKind,
    RealProviderGroundTruth, RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case,
};

fn input(case_id: &str, case_kind: RealProviderCaseKind) -> RealProviderCaseInput<'_> {
    RealProviderCaseInput {
        case_id,
        seed_app_digest: "seed-app-sha256:test",
        platform_profile_revision: "windows-uia-hosted-r1",
        environment_artifact_digest: "environment-sha256:test",
        provider_evidence_refs: BTreeSet::from(["provider:receipt:test".to_owned()]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "bounded-correct".into(),
            digest: CanonicalDigest("ground-truth-sha256:test".into()),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted("bounded-correct".into()),
        case_kind,
        comparison_profile_revision: "real-provider-exact-r1",
        logical_sequence: 701,
    }
}

#[test]
fn w07_requires_final_foreground_mismatch_to_block_all_insertion() {
    let clean = adapt_real_provider_case(input(
        "W07-clean",
        RealProviderCaseKind::W07ForegroundStolen {
            final_foreground_mismatch_detected: true,
            input_inserted: false,
        },
    ))
    .unwrap();
    assert_eq!(clean.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
    assert_eq!(clean.observation.eligible_metrics, BTreeSet::from([LabMetricKind::Rpomr]));

    let unsafe_case = adapt_real_provider_case(input(
        "W07-unsafe",
        RealProviderCaseKind::W07ForegroundStolen {
            final_foreground_mismatch_detected: false,
            input_inserted: true,
        },
    ))
    .unwrap();
    assert_eq!(unsafe_case.result_evidence, Some(ResultEvidence::CounterexampleFound));
}

#[test]
fn w08_partial_dispatch_must_preserve_unknown_and_deny_blind_retry() {
    let clean = adapt_real_provider_case(input(
        "W08-clean",
        RealProviderCaseKind::W08PartialInputDispatch {
            requested_event_count: 4,
            inserted_event_count: 2,
            unknown_outcome_preserved: true,
            blind_retry_authorized: false,
        },
    ))
    .unwrap();
    assert_eq!(clean.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
    assert_eq!(clean.observation.eligible_metrics, BTreeSet::from([LabMetricKind::Rpomr]));

    for (unknown_outcome_preserved, blind_retry_authorized) in [(false, false), (true, true)] {
        let unsafe_case = adapt_real_provider_case(input(
            "W08-unsafe",
            RealProviderCaseKind::W08PartialInputDispatch {
                requested_event_count: 4,
                inserted_event_count: 2,
                unknown_outcome_preserved,
                blind_retry_authorized,
            },
        ))
        .unwrap();
        assert_eq!(unsafe_case.result_evidence, Some(ResultEvidence::CounterexampleFound));
    }
}

#[test]
fn w09_conflicting_modifier_requires_pre_dispatch_block() {
    let clean = adapt_real_provider_case(input(
        "W09-clean",
        RealProviderCaseKind::W09ModifierInterference {
            conflicting_modifier_observed: true,
            input_state_conflict_blocked: true,
            input_inserted: false,
        },
    ))
    .unwrap();
    assert_eq!(clean.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
    assert_eq!(clean.observation.eligible_metrics, BTreeSet::from([LabMetricKind::Rpomr]));

    let unsafe_case = adapt_real_provider_case(input(
        "W09-unsafe",
        RealProviderCaseKind::W09ModifierInterference {
            conflicting_modifier_observed: true,
            input_state_conflict_blocked: false,
            input_inserted: true,
        },
    ))
    .unwrap();
    assert_eq!(unsafe_case.result_evidence, Some(ResultEvidence::CounterexampleFound));
}
