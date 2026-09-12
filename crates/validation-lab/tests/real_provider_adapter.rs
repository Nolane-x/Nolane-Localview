use std::collections::BTreeSet;

use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness,
};
use localview_validation_lab::{
    CanonicalDigest, LabError, LabFailureFlag, LabMetricKind, RealProviderCaseInput,
    RealProviderCaseKind, RealProviderGroundTruth, RealProviderObservedOutcome, ResultEvidence,
    adapt_real_provider_case, derive_real_provider_campaign_evidence, reduce_metric_observations,
};

fn refs(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn truth(outcome: &str, digest: &str) -> RealProviderGroundTruth {
    RealProviderGroundTruth {
        canonical_outcome: outcome.to_owned(),
        digest: CanonicalDigest(digest.to_owned()),
    }
}

fn input<'a>(
    case_id: &'a str,
    observed_outcome: RealProviderObservedOutcome,
    case_kind: RealProviderCaseKind,
) -> RealProviderCaseInput<'a> {
    RealProviderCaseInput {
        case_id,
        seed_app_digest: "seed-app-sha256:abc",
        platform_profile_revision: "windows-uia-hosted-r1",
        environment_artifact_digest: "environment-sha256:def",
        provider_evidence_refs: refs(&["provider:receipt:1"]),
        ground_truth: truth("name=after", "ground-truth-sha256:ghi"),
        observed_outcome,
        case_kind,
        comparison_profile_revision: "real-provider-exact-r1",
        logical_sequence: 301,
    }
}

fn clean_w01() -> RealProviderCaseKind {
    RealProviderCaseKind::W01MissingPropertyEvent {
        continuity: EventContinuityState::GapDetected,
        reconciliation: Some(ReconciliationCompleteness::Established),
        accepted_as_fresh: true,
        accepted_as_reconciled: true,
    }
}

#[test]
fn asserted_provider_world_fact_is_compared_to_independent_ground_truth() {
    let pass = adapt_real_provider_case(input(
        "W01-pass",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        clean_w01(),
    ))
    .unwrap();
    assert!(pass.observation.provider_backed);
    assert!(
        pass.observation
            .eligible_metrics
            .contains(&LabMetricKind::Rpomr)
    );
    assert!(pass.observation.failure_flags.is_empty());
    assert_eq!(
        pass.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert!(
        pass.observation
            .evidence_refs
            .contains("ground-truth:ground-truth-sha256:ghi")
    );
    assert!(
        pass.observation
            .evidence_refs
            .contains("environment:environment-sha256:def")
    );
    assert!(
        pass.observation
            .evidence_refs
            .contains("seed-app:seed-app-sha256:abc")
    );
    assert!(
        pass.observation
            .evidence_refs
            .contains("platform:windows-uia-hosted-r1")
    );

    let mismatch = adapt_real_provider_case(input(
        "W01-mismatch",
        RealProviderObservedOutcome::Asserted("name=before".into()),
        clean_w01(),
    ))
    .unwrap();
    assert_eq!(
        mismatch.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::RealProviderOracleMismatch])
    );
    assert_eq!(
        mismatch.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );

    let snapshot = reduce_metric_observations(&[pass.observation, mismatch.observation]).unwrap();
    let rpomr = snapshot.get(LabMetricKind::Rpomr).unwrap();
    assert_eq!((rpomr.numerator, rpomr.denominator), (1, 2));
}

#[test]
fn conservative_non_assertions_do_not_fake_rpomr_or_real_provider_pass() {
    let records = [
        RealProviderObservedOutcome::Unknown,
        RealProviderObservedOutcome::Inconclusive,
        RealProviderObservedOutcome::Unsupported,
        RealProviderObservedOutcome::ConservativeBlock,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, outcome)| {
        let mut case = input("W01-incomplete", outcome, clean_w01());
        case.logical_sequence += index as u64;
        adapt_real_provider_case(case).unwrap()
    })
    .collect::<Vec<_>>();

    for record in &records {
        assert!(
            !record
                .observation
                .eligible_metrics
                .contains(&LabMetricKind::Rpomr)
        );
        assert!(
            !record
                .observation
                .failure_flags
                .contains(&LabFailureFlag::RealProviderOracleMismatch)
        );
        assert_eq!(record.result_evidence, None);
    }

    let observations = records
        .iter()
        .map(|record| record.observation.clone())
        .collect::<Vec<_>>();
    let snapshot = reduce_metric_observations(&observations).unwrap();
    let rpomr = snapshot.get(LabMetricKind::Rpomr).unwrap();
    assert_eq!((rpomr.numerator, rpomr.denominator), (0, 0));
    assert_eq!(derive_real_provider_campaign_evidence(&records), None);
}

#[test]
fn w01_distinguishes_real_event_gap_from_established_reconciliation() {
    let reconciled = adapt_real_provider_case(input(
        "W01-reconciled",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        clean_w01(),
    ))
    .unwrap();
    assert!(
        reconciled
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Eoffr)
    );
    assert!(
        reconciled
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Rmr)
    );
    assert!(
        !reconciled
            .observation
            .failure_flags
            .contains(&LabFailureFlag::EventOnlyFalseFreshness)
    );
    assert!(
        !reconciled
            .observation
            .failure_flags
            .contains(&LabFailureFlag::ReconciliationMiss)
    );

    let false_fresh = adapt_real_provider_case(input(
        "W01-false-fresh",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W01MissingPropertyEvent {
            continuity: EventContinuityState::GapDetected,
            reconciliation: None,
            accepted_as_fresh: true,
            accepted_as_reconciled: false,
        },
    ))
    .unwrap();
    assert!(
        false_fresh
            .observation
            .failure_flags
            .contains(&LabFailureFlag::EventOnlyFalseFreshness)
    );
    assert_eq!(
        false_fresh.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );

    let reconciliation_miss = adapt_real_provider_case(input(
        "W01-reconciliation-miss",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W01MissingPropertyEvent {
            continuity: EventContinuityState::GapDetected,
            reconciliation: Some(ReconciliationCompleteness::Incomplete),
            accepted_as_fresh: false,
            accepted_as_reconciled: true,
        },
    ))
    .unwrap();
    assert!(
        reconciliation_miss
            .observation
            .failure_flags
            .contains(&LabFailureFlag::ReconciliationMiss)
    );
}

#[test]
fn w02_only_measures_aba_when_real_provider_identity_reuse_was_observed() {
    let escaped = adapt_real_provider_case(input(
        "W02-aba",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W02RecreatedElement {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:windows:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:windows:new"),
            opaque_provider_element_id: "uia-runtime-id:7".into(),
            provider_identity_reuse_observed: true,
            accepted_previous_identity_as_current: true,
        },
    ))
    .unwrap();
    assert!(
        escaped
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Piaer)
    );
    assert!(
        escaped
            .observation
            .failure_flags
            .contains(&LabFailureFlag::ProviderIdAbaEscape)
    );
    assert_eq!(
        escaped.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );

    let no_reuse = adapt_real_provider_case(input(
        "W02-no-reuse",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W02RecreatedElement {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:windows:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:windows:new"),
            opaque_provider_element_id: "uia-runtime-id:9".into(),
            provider_identity_reuse_observed: false,
            accepted_previous_identity_as_current: false,
        },
    ))
    .unwrap();
    assert!(
        !no_reuse
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Piaer)
    );

    let same_provider = adapt_real_provider_case(input(
        "W02-same-incarnation",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W02RecreatedElement {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:windows:same"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:windows:same"),
            opaque_provider_element_id: "uia-runtime-id:7".into(),
            provider_identity_reuse_observed: true,
            accepted_previous_identity_as_current: false,
        },
    ))
    .expect("W02 element recreation can occur within one live provider incarnation");
    assert!(
        same_provider
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Piaer)
    );
    assert!(same_provider.observation.failure_flags.is_empty());
}

#[test]
fn w06_reacquire_tracks_stale_authority_and_cleanup_separately() {
    let record = adapt_real_provider_case(input(
        "W06-reacquire",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        RealProviderCaseKind::W06ProviderReacquire {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:windows:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:windows:new"),
            stale_authority_survived_reacquire: true,
            cleanup_to_baseline: false,
        },
    ))
    .unwrap();
    assert!(
        record
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Scar)
    );
    assert!(
        record
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Cbfr)
    );
    assert!(
        record
            .observation
            .failure_flags
            .contains(&LabFailureFlag::StaleCacheAuthority)
    );
    assert!(
        record
            .observation
            .failure_flags
            .contains(&LabFailureFlag::CleanupToBaselineFailure)
    );
    assert_eq!(
        record.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );
}

#[test]
fn real_provider_authority_fields_fail_closed_before_observation_minting() {
    let mut cases = vec![
        (
            "case_id",
            input(
                "   ",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
        (
            "seed_app_digest",
            input(
                "W01",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
        (
            "platform_profile_revision",
            input(
                "W01",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
        (
            "environment_artifact_digest",
            input(
                "W01",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
        (
            "ground_truth_digest",
            input(
                "W01",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
        (
            "comparison_profile_revision",
            input(
                "W01",
                RealProviderObservedOutcome::Asserted("name=after".into()),
                clean_w01(),
            ),
        ),
    ];
    cases[1].1.seed_app_digest = " ";
    cases[2].1.platform_profile_revision = " ";
    cases[3].1.environment_artifact_digest = " ";
    cases[4].1.ground_truth.digest = CanonicalDigest(" ".into());
    cases[5].1.comparison_profile_revision = " ";

    for (field, case) in cases {
        assert_eq!(
            adapt_real_provider_case(case),
            Err(LabError::EmptyAuthorityField { field })
        );
    }
}

#[test]
fn campaign_evidence_prefers_counterexample_over_incomplete_and_requires_all_passes() {
    let pass_a = adapt_real_provider_case(input(
        "W01-a",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        clean_w01(),
    ))
    .unwrap();
    let mut pass_b_input = input(
        "W01-b",
        RealProviderObservedOutcome::Asserted("name=after".into()),
        clean_w01(),
    );
    pass_b_input.logical_sequence = 302;
    let pass_b = adapt_real_provider_case(pass_b_input).unwrap();
    assert_eq!(
        derive_real_provider_campaign_evidence(&[pass_a.clone(), pass_b.clone()]),
        Some(ResultEvidence::RealProviderIntegrationPass)
    );

    let incomplete = adapt_real_provider_case(input(
        "W01-incomplete",
        RealProviderObservedOutcome::Unknown,
        clean_w01(),
    ))
    .unwrap();
    assert_eq!(
        derive_real_provider_campaign_evidence(&[pass_a.clone(), incomplete.clone()]),
        None
    );

    let mismatch = adapt_real_provider_case(input(
        "W01-counterexample",
        RealProviderObservedOutcome::Asserted("name=before".into()),
        clean_w01(),
    ))
    .unwrap();
    assert_eq!(
        derive_real_provider_campaign_evidence(&[pass_a, incomplete, mismatch]),
        Some(ResultEvidence::CounterexampleFound)
    );
}
