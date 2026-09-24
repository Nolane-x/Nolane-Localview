use std::collections::BTreeSet;

use localview_validation_lab::{
    CanonicalDigest, LabMetricKind, RealProviderCaseInput, RealProviderCaseKind,
    RealProviderGroundTruth, RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case,
};

fn input(case_id: &str, case_kind: RealProviderCaseKind) -> RealProviderCaseInput<'_> {
    RealProviderCaseInput {
        case_id,
        seed_app_digest: "seed-app-sha256:w10-test",
        platform_profile_revision: "windows-uia-mixed-dpi-r1",
        environment_artifact_digest: "environment-sha256:w10-test",
        provider_evidence_refs: BTreeSet::from([
            "provider:w10:physical-geometry-receipt-a".to_owned(),
            "provider:w10:physical-geometry-receipt-b".to_owned(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "mixed-dpi-physical-geometry-matched".into(),
            digest: CanonicalDigest("ground-truth-sha256:w10-test".into()),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "mixed-dpi-physical-geometry-matched".into(),
        ),
        case_kind,
        comparison_profile_revision: "real-provider-exact-r1",
        logical_sequence: 110,
    }
}

fn clean_case() -> RealProviderCaseKind {
    RealProviderCaseKind::W10MixedDpiGeometry {
        distinct_effective_dpi_observed: true,
        coordinate_space_explicit: true,
        first_rect_matches_oracle: true,
        second_rect_matches_oracle: true,
        double_scaling_observed: false,
    }
}

#[test]
fn w10_requires_distinct_real_dpi_and_exact_physical_geometry() {
    let clean = adapt_real_provider_case(input("W10-clean", clean_case())).unwrap();
    assert_eq!(
        clean.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
    assert_eq!(
        clean.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Rpomr])
    );

    let unsafe_cases = [
        RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed: false,
            coordinate_space_explicit: true,
            first_rect_matches_oracle: true,
            second_rect_matches_oracle: true,
            double_scaling_observed: false,
        },
        RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed: true,
            coordinate_space_explicit: false,
            first_rect_matches_oracle: true,
            second_rect_matches_oracle: true,
            double_scaling_observed: false,
        },
        RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed: true,
            coordinate_space_explicit: true,
            first_rect_matches_oracle: false,
            second_rect_matches_oracle: true,
            double_scaling_observed: false,
        },
        RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed: true,
            coordinate_space_explicit: true,
            first_rect_matches_oracle: true,
            second_rect_matches_oracle: false,
            double_scaling_observed: false,
        },
        RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed: true,
            coordinate_space_explicit: true,
            first_rect_matches_oracle: true,
            second_rect_matches_oracle: true,
            double_scaling_observed: true,
        },
    ];

    for case_kind in unsafe_cases {
        let unsafe_case = adapt_real_provider_case(input("W10-unsafe", case_kind)).unwrap();
        assert_eq!(
            unsafe_case.result_evidence,
            Some(ResultEvidence::CounterexampleFound)
        );
    }
}
