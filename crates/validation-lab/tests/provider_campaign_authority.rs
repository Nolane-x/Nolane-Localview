use std::collections::BTreeSet;

use localview_validation_lab::{
    ActualExecutionAuthority, CampaignLayer, LabError, LabFailureFlag, LabMetricKind,
    LabObservation, LabPreregistration, LabRevisionContext, LabRunAdmission, LabRunBuilder,
    LabSeedIdentity, PersistedPreregistrationReceipt, ProviderCampaignKind, ResultEvidence,
    canonical_digest, validate_persisted_receipt, validate_provider_campaign_layer,
};
use serde_json::json;

fn preregistration(layer: CampaignLayer, platform_profile: Option<&str>) -> LabPreregistration {
    LabPreregistration {
        revision_context: LabRevisionContext {
            lab_revision: "lab-v43-real-provider-r1".into(),
            seed_corpus_revision: "windows-provider-seeds-r1".into(),
            spec_revision_digest: "v4.3".into(),
            reference_reducer_revision: "provider-oracle-r1".into(),
            mutation_catalog_revision: "mutation-r2".into(),
            comparison_profile_revision: "real-provider-exact-r1".into(),
            random_source_profile: "deterministic".into(),
            platform_profile: platform_profile.map(str::to_owned),
            start_sequence: 20,
        },
        seed_catalog_digest: canonical_digest(&json!({"catalog": "windows-provider-seeds-r1"}))
            .unwrap(),
        seed_identities: vec![LabSeedIdentity {
            seed_id: "W01".into(),
            prediction_revision: "pred-r1".into(),
            oracle_revision: "oracle-r1".into(),
        }],
        campaign_layer: layer,
        expected_distinctions: BTreeSet::from([
            "event continuity != current snapshot completeness".into(),
        ]),
        model_bound: None,
        assumptions: BTreeSet::from(["hosted Windows UIA seed".into()]),
        declared_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
        creation_sequence: 9,
    }
}

fn actual_authority(prereg: &LabPreregistration) -> ActualExecutionAuthority {
    ActualExecutionAuthority {
        seed_catalog_digest: prereg.seed_catalog_digest.clone(),
        comparison_profile_revision: prereg.revision_context.comparison_profile_revision.clone(),
        random_source_profile: prereg.revision_context.random_source_profile.clone(),
        model_bound: prereg.model_bound,
    }
}

fn prospective_admission(prereg: &LabPreregistration) -> LabRunAdmission {
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
    LabRunAdmission::Prospective {
        preregistration: prereg.clone(),
        receipt,
    }
}

fn rpomr_observation(failure: bool) -> LabObservation {
    LabObservation {
        observation_id: "real-provider:W01".into(),
        seed_id: Some("W01".into()),
        expected_outcome: "name=after".into(),
        observed_outcome: if failure {
            "name=before".into()
        } else {
            "name=after".into()
        },
        principal_expected: None,
        principal_dispatched: None,
        eligible_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
        failure_flags: if failure {
            BTreeSet::from([LabFailureFlag::RealProviderOracleMismatch])
        } else {
            BTreeSet::new()
        },
        evidence_refs: BTreeSet::from([
            "ground-truth:digest-1".into(),
            "environment:digest-2".into(),
        ]),
        provider_backed: true,
        comparison_profile_revision: "real-provider-exact-r1".into(),
        logical_sequence: 21,
    }
}

#[test]
fn provider_campaign_layers_are_semantically_fixed() {
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::FakeProviderSimulator,
        CampaignLayer::L6,
    )
    .is_ok());
    assert!(validate_provider_campaign_layer(
        ProviderCampaignKind::RealProviderSeedApplications,
        CampaignLayer::L7,
    )
    .is_ok());

    assert_eq!(
        validate_provider_campaign_layer(
            ProviderCampaignKind::FakeProviderSimulator,
            CampaignLayer::L7,
        ),
        Err(LabError::ProviderCampaignLayerMismatch {
            campaign: ProviderCampaignKind::FakeProviderSimulator,
            expected: CampaignLayer::L6,
            actual: CampaignLayer::L7,
        })
    );
    assert_eq!(
        validate_provider_campaign_layer(
            ProviderCampaignKind::RealProviderSeedApplications,
            CampaignLayer::L6,
        ),
        Err(LabError::ProviderCampaignLayerMismatch {
            campaign: ProviderCampaignKind::RealProviderSeedApplications,
            expected: CampaignLayer::L7,
            actual: CampaignLayer::L6,
        })
    );
}

#[test]
fn real_provider_campaign_requires_prospective_l7_admission() {
    let l6 = preregistration(CampaignLayer::L6, Some("windows-uia-r1"));
    assert_eq!(
        LabRunBuilder::start_provider_campaign(
            ProviderCampaignKind::RealProviderSeedApplications,
            prospective_admission(&l6),
            actual_authority(&l6),
        )
        .unwrap_err(),
        LabError::ProviderCampaignLayerMismatch {
            campaign: ProviderCampaignKind::RealProviderSeedApplications,
            expected: CampaignLayer::L7,
            actual: CampaignLayer::L6,
        }
    );

    let l7 = preregistration(CampaignLayer::L7, Some("windows-uia-r1"));
    let exploratory = LabRunAdmission::Exploratory {
        revision_context: l7.revision_context.clone(),
        seed_identities: l7.seed_identities.clone(),
        assumptions: l7.assumptions.clone(),
        downgrade_reason: localview_validation_lab::DowngradeReason::CallerSelectedExploratory,
    };
    assert_eq!(
        LabRunBuilder::start_provider_campaign(
            ProviderCampaignKind::RealProviderSeedApplications,
            exploratory,
            actual_authority(&l7),
        )
        .unwrap_err(),
        LabError::ProviderCampaignRequiresProspectiveAdmission
    );
}

#[test]
fn generic_run_cannot_self_promote_to_real_provider_pass() {
    let prereg = preregistration(CampaignLayer::L7, Some("windows-uia-r1"));
    let mut run = LabRunBuilder::start(
        prospective_admission(&prereg),
        actual_authority(&prereg),
    )
    .unwrap();
    run.append_observation(rpomr_observation(false)).unwrap();

    assert_eq!(
        run.finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_requires_typed_l7_campaign_admission"
        }
    );
}

#[test]
fn typed_l7_run_cannot_pass_without_measured_clean_rpomr() {
    let prereg = preregistration(CampaignLayer::L7, Some("windows-uia-r1"));
    let mut empty = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        prospective_admission(&prereg),
        actual_authority(&prereg),
    )
    .unwrap();
    assert_eq!(
        empty
            .finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_requires_measured_rpomr"
        }
    );

    let mut mismatch = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        prospective_admission(&prereg),
        actual_authority(&prereg),
    )
    .unwrap();
    mismatch.append_observation(rpomr_observation(true)).unwrap();
    assert_eq!(
        mismatch
            .finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_requires_zero_rpomr_mismatches"
        }
    );
}

#[test]
fn typed_l7_run_requires_platform_profile_and_provider_backed_rpomr_evidence() {
    let missing_platform = preregistration(CampaignLayer::L7, None);
    let mut no_platform = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        prospective_admission(&missing_platform),
        actual_authority(&missing_platform),
    )
    .unwrap();
    assert_eq!(
        no_platform.append_observation(rpomr_observation(false)),
        Err(LabError::ProviderBackedObservationRequiresPlatformProfile)
    );

    let prereg = preregistration(CampaignLayer::L7, Some("windows-uia-r1"));
    let mut run = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        prospective_admission(&prereg),
        actual_authority(&prereg),
    )
    .unwrap();
    let mut non_provider = rpomr_observation(false);
    non_provider.provider_backed = false;
    run.append_observation(non_provider).unwrap();
    assert_eq!(
        run.finalize(ResultEvidence::RealProviderIntegrationPass, 30)
            .unwrap_err(),
        LabError::InvalidRealProviderPass {
            reason: "real_provider_pass_requires_provider_backed_rpomr_observations"
        }
    );
}

#[test]
fn typed_l7_clean_rpomr_can_mint_scoped_real_provider_pass() {
    let prereg = preregistration(CampaignLayer::L7, Some("windows-uia-r1"));
    let mut run = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        prospective_admission(&prereg),
        actual_authority(&prereg),
    )
    .unwrap();
    run.append_observation(rpomr_observation(false)).unwrap();

    let completed = run
        .finalize(ResultEvidence::RealProviderIntegrationPass, 30)
        .unwrap();
    let rpomr = completed
        .payload
        .metric_snapshot
        .get(LabMetricKind::Rpomr)
        .unwrap();
    assert_eq!((rpomr.numerator, rpomr.denominator), (0, 1));
    assert_eq!(
        completed.payload.result_class,
        localview_validation_lab::ResearchResultClass::RealProviderIntegrationPass
    );
}
