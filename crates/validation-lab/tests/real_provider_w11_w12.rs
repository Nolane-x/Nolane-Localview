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
        logical_sequence: 1112,
    }
}

#[test]
fn w11_requires_real_modal_block_and_zero_input_or_target_effect() {
    let clean = adapt_real_provider_case(input(
        "W11-clean",
        RealProviderCaseKind::W11ModalBeforeDispatch {
            modal_blocker_observed: true,
            input_inserted: false,
            target_effect_observed: false,
        },
    ))
    .unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    for (modal_blocker_observed, input_inserted, target_effect_observed) in [
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let unsafe_case = adapt_real_provider_case(input(
            "W11-unsafe",
            RealProviderCaseKind::W11ModalBeforeDispatch {
                modal_blocker_observed,
                input_inserted,
                target_effect_observed,
            },
        ))
        .unwrap();
        assert_eq!(
            unsafe_case.result_evidence,
            Some(ResultEvidence::CounterexampleFound)
        );
    }
}

#[test]
fn w12_requires_stale_authority_rejection_and_fresh_reacquire() {
    let clean = adapt_real_provider_case(input(
        "W12-clean",
        RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: true,
            replacement_target_present: true,
            stale_authority_rejected: true,
            replacement_effect_observed: false,
            fresh_reacquire_required: true,
        },
    ))
    .unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    let unsafe_cases = [
        RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: false,
            replacement_target_present: true,
            stale_authority_rejected: true,
            replacement_effect_observed: false,
            fresh_reacquire_required: true,
        },
        RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: true,
            replacement_target_present: true,
            stale_authority_rejected: false,
            replacement_effect_observed: false,
            fresh_reacquire_required: true,
        },
        RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: true,
            replacement_target_present: true,
            stale_authority_rejected: true,
            replacement_effect_observed: true,
            fresh_reacquire_required: true,
        },
        RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: true,
            replacement_target_present: true,
            stale_authority_rejected: true,
            replacement_effect_observed: false,
            fresh_reacquire_required: false,
        },
    ];

    for case_kind in unsafe_cases {
        let unsafe_case = adapt_real_provider_case(input("W12-unsafe", case_kind)).unwrap();
        assert_eq!(
            unsafe_case.result_evidence,
            Some(ResultEvidence::CounterexampleFound)
        );
    }
}
