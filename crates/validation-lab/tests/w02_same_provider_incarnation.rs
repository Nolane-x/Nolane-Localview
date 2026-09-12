use std::collections::BTreeSet;

use localview_protocol::ProviderIncarnationRef;
use localview_validation_lab::{
    CanonicalDigest, LabMetricKind, RealProviderCaseInput, RealProviderCaseKind,
    RealProviderGroundTruth, RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case,
};

#[test]
fn w02_element_recreation_can_measure_aba_within_one_provider_incarnation() {
    let provider_incarnation = ProviderIncarnationRef::from("provider:windows:stable-worker");
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W02-same-provider-element-recreation",
        seed_app_digest: "seed-app-sha256:w02",
        platform_profile_revision: "windows-uia-hosted-r1",
        environment_artifact_digest: "environment-sha256:w02",
        provider_evidence_refs: BTreeSet::from(["provider:receipt:w02".to_owned()]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "stale-element-ref-rejected".into(),
            digest: CanonicalDigest("ground-truth-sha256:w02".into()),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "stale-element-ref-rejected".into(),
        ),
        case_kind: RealProviderCaseKind::W02RecreatedElement {
            previous_provider_incarnation: provider_incarnation.clone(),
            current_provider_incarnation: provider_incarnation,
            opaque_provider_element_id: "uia-runtime:[42,7]".into(),
            provider_identity_reuse_observed: true,
            accepted_previous_identity_as_current: false,
        },
        comparison_profile_revision: "real-provider-exact-r1",
        logical_sequence: 402,
    })
    .expect("W02 element ABA is meaningful inside one live provider incarnation");

    assert!(
        record
            .observation
            .eligible_metrics
            .contains(&LabMetricKind::Piaer)
    );
    assert!(record.observation.failure_flags.is_empty());
    assert_eq!(
        record.result_evidence,
        Some(ResultEvidence::RealProviderIntegrationPass)
    );
}
