use std::collections::BTreeSet;

use localview_validation_lab::{
    ActualExecutionAuthority, CampaignLayer, LabError, LabMetricKind, LabObservation,
    LabPreregistration, LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
    PersistedPreregistrationReceipt, ProviderCampaignKind, ResultEvidence, canonical_digest,
    validate_persisted_receipt,
};
use serde_json::json;

fn preregistration() -> LabPreregistration {
    let seed_ids = [
        "W01-missing-uia-property-event",
        "W02-recreated-uia-element",
        "W06-windows-uia-provider-reacquire",
    ];
    LabPreregistration {
        revision_context: LabRevisionContext {
            lab_revision: "lab-v43-real-provider-seed-coverage-r1".into(),
            seed_corpus_revision: "windows-provider-seeds-w01-w02-w06-r1".into(),
            spec_revision_digest: "v4.3".into(),
            reference_reducer_revision: "provider-oracle-r1".into(),
            mutation_catalog_revision: "windows-provider-seed-matrix-r1".into(),
            comparison_profile_revision: "real-provider-exact-r1".into(),
            random_source_profile: "deterministic".into(),
            platform_profile: Some("windows-uia-r1".into()),
            start_sequence: 20,
        },
        seed_catalog_digest: canonical_digest(&json!({"seeds": seed_ids})).unwrap(),
        seed_identities: seed_ids
            .into_iter()
            .map(|seed_id| LabSeedIdentity {
                seed_id: seed_id.into(),
                prediction_revision: format!("prediction:{seed_id}:r1"),
                oracle_revision: "independent-seed-oracle-r1".into(),
            })
            .collect(),
        campaign_layer: CampaignLayer::L7,
        expected_distinctions: BTreeSet::from([
            "required real-provider seeds must all execute".into(),
        ]),
        model_bound: None,
        assumptions: BTreeSet::from(["hosted Windows UIA seed".into()]),
        declared_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
        creation_sequence: 9,
    }
}

fn authority(preregistration: &LabPreregistration) -> ActualExecutionAuthority {
    ActualExecutionAuthority {
        seed_catalog_digest: preregistration.seed_catalog_digest.clone(),
        comparison_profile_revision: preregistration
            .revision_context
            .comparison_profile_revision
            .clone(),
        random_source_profile: preregistration
            .revision_context
            .random_source_profile
            .clone(),
        model_bound: None,
    }
}

fn admission(preregistration: &LabPreregistration) -> LabRunAdmission {
    let prepared = preregistration.prepare().unwrap();
    let receipt = validate_persisted_receipt(
        &prepared,
        PersistedPreregistrationReceipt {
            digest: prepared.digest.clone(),
            logical_sequence: 11,
            persistence_ref: "LAB-PREREGISTRATION.json#11".into(),
        },
    )
    .unwrap();
    LabRunAdmission::Prospective {
        preregistration: preregistration.clone(),
        receipt,
    }
}

fn clean_observation(seed_id: &str, logical_sequence: u64) -> LabObservation {
    LabObservation {
        observation_id: format!("real-provider:{seed_id}"),
        seed_id: Some(seed_id.into()),
        expected_outcome: "oracle=clean".into(),
        observed_outcome: "oracle=clean".into(),
        principal_expected: None,
        principal_dispatched: None,
        eligible_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
        failure_flags: BTreeSet::new(),
        evidence_refs: BTreeSet::from([
            format!("ground-truth:{seed_id}"),
            "environment:windows-uia-r1".into(),
        ]),
        provider_backed: true,
        comparison_profile_revision: "real-provider-exact-r1".into(),
        logical_sequence,
    }
}

fn l7_run(preregistration: &LabPreregistration) -> LabRunBuilder {
    LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        admission(preregistration),
        authority(preregistration),
    )
    .unwrap()
}

#[test]
fn real_provider_pass_requires_every_preregistered_seed_to_be_observed() {
    let preregistration = preregistration();
    let mut run = l7_run(&preregistration);
    run.append_observation(clean_observation(
        "W01-missing-uia-property-event",
        21,
    ))
    .unwrap();
    run.append_observation(clean_observation(
        "W02-recreated-uia-element",
        22,
    ))
    .unwrap();

    assert_eq!(
        run.finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_requires_all_preregistered_seeds"
        }
    );
}

#[test]
fn real_provider_pass_rejects_observations_from_unregistered_seeds() {
    let preregistration = preregistration();
    let mut run = l7_run(&preregistration);
    run.append_observation(clean_observation(
        "W01-missing-uia-property-event",
        21,
    ))
    .unwrap();
    run.append_observation(clean_observation(
        "W02-recreated-uia-element",
        22,
    ))
    .unwrap();
    run.append_observation(clean_observation(
        "W06-windows-uia-provider-reacquire",
        23,
    ))
    .unwrap();
    run.append_observation(clean_observation("W99-unregistered", 24))
        .unwrap();

    assert_eq!(
        run.finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_rejects_unregistered_seed_observations"
        }
    );
}
