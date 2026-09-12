use std::collections::BTreeSet;

use localview_validation_lab::{
    ActualExecutionAuthority, CampaignLayer, LabError, LabFailureFlag, LabMetricKind, LabObservation,
    LabPreregistration, LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
    PersistedPreregistrationReceipt, ResultEvidence, canonical_digest, validate_persisted_receipt,
};
use serde_json::json;

fn preregistration(platform_profile: Option<&str>) -> LabPreregistration {
    LabPreregistration {
        revision_context: LabRevisionContext {
            lab_revision: "lab-r6b".into(),
            seed_corpus_revision: "corpus-r6b".into(),
            spec_revision_digest: "spec-r9".into(),
            reference_reducer_revision: "reducer-r3".into(),
            mutation_catalog_revision: "mutation-r2".into(),
            comparison_profile_revision: "compare-r7".into(),
            random_source_profile: "rng-fixed-41".into(),
            platform_profile: platform_profile.map(str::to_owned),
            start_sequence: 20,
        },
        seed_catalog_digest: canonical_digest(&json!({"catalog": "r6b"})).unwrap(),
        seed_identities: vec![LabSeedIdentity {
            seed_id: "LV-S061".into(),
            prediction_revision: "pred-r1".into(),
            oracle_revision: "oracle-r1".into(),
        }],
        campaign_layer: CampaignLayer::L1,
        expected_distinctions: BTreeSet::from(["CURRENT != STALE".into()]),
        model_bound: Some(64),
        assumptions: BTreeSet::from(["deterministic fixture".into()]),
        declared_metrics: BTreeSet::from([LabMetricKind::Suar]),
        creation_sequence: 9,
    }
}

fn run(prereg: &LabPreregistration) -> LabRunBuilder {
    let prepared = prereg.prepare().unwrap();
    let receipt = validate_persisted_receipt(
        &prepared,
        PersistedPreregistrationReceipt {
            digest: prepared.digest.clone(),
            logical_sequence: 11,
            persistence_ref: "LAB-PREREGISTRATION.json#11".into(),
        },
    )
    .unwrap();
    let actual = ActualExecutionAuthority {
        seed_catalog_digest: prereg.seed_catalog_digest.clone(),
        comparison_profile_revision: prereg.revision_context.comparison_profile_revision.clone(),
        random_source_profile: prereg.revision_context.random_source_profile.clone(),
        model_bound: prereg.model_bound,
    };
    LabRunBuilder::start(
        LabRunAdmission::Prospective {
            preregistration: prereg.clone(),
            receipt,
        },
        actual,
    )
    .unwrap()
}

fn observation(
    id: &str,
    sequence: u64,
    comparison_profile_revision: &str,
    provider_backed: bool,
    eligible_metrics: BTreeSet<LabMetricKind>,
) -> LabObservation {
    LabObservation {
        observation_id: id.into(),
        seed_id: Some("LV-S061".into()),
        expected_outcome: "CURRENT".into(),
        observed_outcome: "CURRENT".into(),
        principal_expected: None,
        principal_dispatched: None,
        eligible_metrics,
        failure_flags: BTreeSet::<LabFailureFlag>::new(),
        evidence_refs: BTreeSet::from([format!("evidence:{id}")]),
        provider_backed,
        comparison_profile_revision: comparison_profile_revision.into(),
        logical_sequence: sequence,
    }
}

#[test]
fn provider_backed_prospective_observation_requires_platform_profile() {
    let prereg = preregistration(None);
    let mut run = run(&prereg);

    assert_eq!(
        run.append_observation(observation(
            "provider-backed",
            21,
            "compare-r7",
            true,
            BTreeSet::from([LabMetricKind::Suar]),
        )),
        Err(LabError::ProviderBackedObservationRequiresPlatformProfile),
    );
}

#[test]
fn observation_comparison_profile_must_match_admitted_authority() {
    let prereg = preregistration(None);
    let mut run = run(&prereg);

    assert_eq!(
        run.append_observation(observation(
            "comparison-drift",
            21,
            "compare-other",
            false,
            BTreeSet::from([LabMetricKind::Suar]),
        )),
        Err(LabError::ObservationAuthorityDrift {
            field: "comparison_profile_revision",
        }),
    );
}

#[test]
fn observation_sequence_must_be_after_start_and_strictly_increase() {
    let prereg = preregistration(None);
    let mut run = run(&prereg);

    assert_eq!(
        run.append_observation(observation(
            "at-start",
            20,
            "compare-r7",
            false,
            BTreeSet::from([LabMetricKind::Suar]),
        )),
        Err(LabError::ObservationSequenceNotAfterStart {
            logical_sequence: 20,
            start_sequence: 20,
        }),
    );

    run.append_observation(observation(
        "first",
        21,
        "compare-r7",
        false,
        BTreeSet::from([LabMetricKind::Suar]),
    ))
    .unwrap();
    assert_eq!(
        run.append_observation(observation(
            "duplicate-sequence",
            21,
            "compare-r7",
            false,
            BTreeSet::from([LabMetricKind::Suar]),
        )),
        Err(LabError::ObservationSequenceNotMonotonic {
            previous_sequence: 21,
            logical_sequence: 21,
        }),
    );
}

#[test]
fn prospective_observation_cannot_measure_undeclared_metric() {
    let prereg = preregistration(None);
    let mut run = run(&prereg);

    assert_eq!(
        run.append_observation(observation(
            "undeclared-wpdr",
            21,
            "compare-r7",
            false,
            BTreeSet::from([LabMetricKind::Wpdr]),
        )),
        Err(LabError::ObservationUsesUndeclaredMetric {
            metric: LabMetricKind::Wpdr,
        }),
    );
}

#[test]
fn rejected_observation_does_not_advance_sequence_or_metric_authority() {
    let prereg = preregistration(None);
    let mut run = run(&prereg);

    assert!(matches!(
        run.append_observation(observation(
            "bad-comparison",
            21,
            "compare-other",
            false,
            BTreeSet::from([LabMetricKind::Suar]),
        )),
        Err(LabError::ObservationAuthorityDrift { .. })
    ));

    run.append_observation(observation(
        "good-after-reject",
        21,
        "compare-r7",
        false,
        BTreeSet::from([LabMetricKind::Suar]),
    ))
    .unwrap();
    let completed = run.finalize(ResultEvidence::PreregisteredSeedPass, 30).unwrap();
    let suar = completed.payload.metric_snapshot.get(LabMetricKind::Suar).unwrap();
    assert_eq!((suar.numerator, suar.denominator), (0, 1));
    assert_eq!(completed.payload.observation_digests.len(), 1);
}
