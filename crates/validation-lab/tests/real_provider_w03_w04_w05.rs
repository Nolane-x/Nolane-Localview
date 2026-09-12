use std::collections::BTreeSet;

use localview_validation_lab::{
    CanonicalDigest, LabFailureFlag, LabMetricKind, RealProviderCaseInput, RealProviderCaseKind,
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
        logical_sequence: 401,
    }
}

#[test]
fn w03_requires_blocked_placeholder_and_fresh_realized_cut_without_synthetic_metric() {
    let clean = adapt_real_provider_case(input(
        "W03-clean",
        RealProviderCaseKind::W03VirtualizedItemRealization {
            placeholder_blocked_before_realization: true,
            fresh_cut_after_realization: true,
            realized_current_after_fresh_cut: true,
        },
    ))
    .unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(clean.observation.failure_flags, BTreeSet::new());
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    let stale = adapt_real_provider_case(input(
        "W03-no-fresh-cut",
        RealProviderCaseKind::W03VirtualizedItemRealization {
            placeholder_blocked_before_realization: true,
            fresh_cut_after_realization: false,
            realized_current_after_fresh_cut: false,
        },
    ))
    .unwrap();
    assert_eq!(
        stale.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );
    assert!(stale.observation.failure_flags.is_empty());
    assert_eq!(
        stale.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );
}

#[test]
fn w04_typed_unsupported_blocks_dispatch_without_inventing_a_metric() {
    let clean = adapt_real_provider_case(input(
        "W04-clean",
        RealProviderCaseKind::W04UnsupportedInvoke {
            invoke_support_unsupported: true,
            dispatch_attempted: false,
            side_effect_observed: false,
        },
    ))
    .unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(clean.observation.failure_flags, BTreeSet::new());
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    let dispatched = adapt_real_provider_case(input(
        "W04-dispatch-escape",
        RealProviderCaseKind::W04UnsupportedInvoke {
            invoke_support_unsupported: true,
            dispatch_attempted: true,
            side_effect_observed: true,
        },
    ))
    .unwrap();
    assert_eq!(
        dispatched.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );
    assert!(dispatched.observation.failure_flags.is_empty());
    assert_eq!(
        dispatched.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );
}

#[test]
fn w05_requires_bounded_poison_nonreuse_and_reacquire_without_fake_uobrr_or_cbfr() {
    let clean = adapt_real_provider_case(input(
        "W05-clean",
        RealProviderCaseKind::W05ProviderHang {
            caller_returned_bounded: true,
            poisoned_worker_reused: false,
            provider_reacquired: true,
            stale_authority_survived_reacquire: false,
        },
    ))
    .unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(clean.observation.failure_flags, BTreeSet::new());
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    let poisoned_reuse = adapt_real_provider_case(input(
        "W05-poison-reuse",
        RealProviderCaseKind::W05ProviderHang {
            caller_returned_bounded: false,
            poisoned_worker_reused: true,
            provider_reacquired: false,
            stale_authority_survived_reacquire: false,
        },
    ))
    .unwrap();
    assert_eq!(
        poisoned_reuse.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );
    assert!(
        !poisoned_reuse
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Uobrr)
    );
    assert!(
        !poisoned_reuse
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Cbfr)
    );
    assert!(poisoned_reuse.observation.failure_flags.is_empty());

    let stale_authority = adapt_real_provider_case(input(
        "W05-stale-authority",
        RealProviderCaseKind::W05ProviderHang {
            caller_returned_bounded: true,
            poisoned_worker_reused: false,
            provider_reacquired: true,
            stale_authority_survived_reacquire: true,
        },
    ))
    .unwrap();
    assert_eq!(
        stale_authority.result_evidence,
        Some(ResultEvidence::CounterexampleFound)
    );
    assert!(
        stale_authority
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Scar)
    );
    assert_eq!(
        stale_authority.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::StaleCacheAuthority])
    );
}
