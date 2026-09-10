use std::collections::BTreeSet;

use localview_validation_lab::{
    ActualExecutionAuthority, CampaignLayer, DowngradeReason, ExecutionMode, LabError, LabMetricKind,
    LabPreregistration, LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
    PersistedPreregistrationReceipt, PreregistrationReceiptProjection, ResearchResultClass,
    ResultEvidence, canonical_digest, validate_persisted_receipt,
};
use serde_json::json;

fn revision_context() -> LabRevisionContext {
    LabRevisionContext {
        lab_revision: "lab-r5".into(),
        seed_corpus_revision: "corpus-r4".into(),
        spec_revision_digest: "spec-r9".into(),
        reference_reducer_revision: "reducer-r3".into(),
        mutation_catalog_revision: "mutation-r2".into(),
        comparison_profile_revision: "compare-r7".into(),
        random_source_profile: "rng-fixed-41".into(),
        platform_profile: None,
        start_sequence: 20,
    }
}

fn preregistration() -> LabPreregistration {
    LabPreregistration {
        revision_context: revision_context(),
        seed_catalog_digest: canonical_digest(&json!({"catalog": "r4"})).unwrap(),
        seed_identities: vec![LabSeedIdentity {
            seed_id: "LV-S041".into(),
            prediction_revision: "pred-r2".into(),
            oracle_revision: "oracle-r3".into(),
        }],
        campaign_layer: CampaignLayer::L1,
        expected_distinctions: BTreeSet::from(["STALE != CURRENT".into()]),
        model_bound: Some(128),
        assumptions: BTreeSet::from(["deterministic fixture".into()]),
        declared_metrics: BTreeSet::from([LabMetricKind::Suar]),
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

fn validated_receipt(
    prereg: &LabPreregistration,
    logical_sequence: u64,
) -> localview_validation_lab::ValidatedPreregistrationReceipt {
    let prepared = prereg.prepare().unwrap();
    validate_persisted_receipt(
        &prepared,
        PersistedPreregistrationReceipt {
            digest: prepared.digest.clone(),
            logical_sequence,
            persistence_ref: format!("LAB-PREREGISTRATION.json#{logical_sequence}"),
        },
    )
    .unwrap()
}

fn prospective_admission(
    prereg: &LabPreregistration,
) -> localview_validation_lab::LabRunAdmission {
    LabRunAdmission::Prospective {
        preregistration: prereg.clone(),
        receipt: validated_receipt(prereg, 11),
    }
}

#[test]
fn result_evidence_maps_to_a_closed_non_proved_taxonomy() {
    let cases = [
        (ResultEvidence::ExploratoryObservation, ResearchResultClass::ExploratoryObservation),
        (ResultEvidence::PreregisteredSeedPass, ResearchResultClass::PreregisteredSeedPass),
        (ResultEvidence::CounterexampleFound, ResearchResultClass::CounterexampleFound),
        (
            ResultEvidence::NoCounterexampleWithinBoundN,
            ResearchResultClass::NoCounterexampleWithinBoundN,
        ),
        (ResultEvidence::MutantKilled, ResearchResultClass::MutantKilled),
        (ResultEvidence::MutantSurvived, ResearchResultClass::MutantSurvived),
        (
            ResultEvidence::DifferentialEquivalentWithinVectorSet,
            ResearchResultClass::DifferentialEquivalentWithinVectorSet,
        ),
        (
            ResultEvidence::DifferentialDivergenceFound,
            ResearchResultClass::DifferentialDivergenceFound,
        ),
        (
            ResultEvidence::PropertyCampaignPassN,
            ResearchResultClass::PropertyCampaignPassN,
        ),
        (
            ResultEvidence::RealProviderIntegrationPass,
            ResearchResultClass::RealProviderIntegrationPass,
        ),
        (
            ResultEvidence::IndependentReplicationPass,
            ResearchResultClass::IndependentReplicationPass,
        ),
    ];

    for (evidence, expected) in cases {
        assert_eq!(evidence.class(), expected);
        assert!(!serde_json::to_string(&expected).unwrap().contains("proved"));
    }
}

#[test]
fn exact_validated_receipt_and_actual_authority_admit_prospective_seed_pass() {
    let prereg = preregistration();
    let receipt = validated_receipt(&prereg, 11);
    let expected_digest = receipt.digest().clone();
    let expected_sequence = receipt.logical_sequence();
    let expected_ref = receipt.persistence_ref().to_owned();

    let mut run = LabRunBuilder::start(
        LabRunAdmission::Prospective {
            preregistration: prereg.clone(),
            receipt,
        },
        actual_authority(&prereg),
    )
    .unwrap();
    run.append_observation_digest(canonical_digest(&json!({"observation": 1})).unwrap())
        .unwrap();

    let completed = run.finalize(ResultEvidence::PreregisteredSeedPass, 30).unwrap();
    assert_eq!(completed.payload.result_class, ResearchResultClass::PreregisteredSeedPass);
    assert_eq!(completed.payload.preregistration_digest, Some(expected_digest.clone()));
    assert_eq!(completed.identity.result_artifact_digest, canonical_digest(&completed.payload).unwrap());

    match &completed.payload.execution_mode {
        ExecutionMode::Prospective { receipt } => {
            assert_eq!(receipt.digest, expected_digest);
            assert_eq!(receipt.logical_sequence, expected_sequence);
            assert_eq!(receipt.persistence_ref, expected_ref);
        }
        other => panic!("expected prospective result mode, got {other:?}"),
    }
}

#[test]
fn receipt_projection_round_trips_as_data_without_becoming_live_authority() {
    let prereg = preregistration();
    let validated = validated_receipt(&prereg, 11);
    let projection = PreregistrationReceiptProjection::from(&validated);

    let encoded = serde_json::to_vec(&projection).unwrap();
    let decoded: PreregistrationReceiptProjection = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, projection);
}

#[test]
fn exploratory_admission_does_not_require_a_persisted_preregistration_or_escalate() {
    let prereg = preregistration();
    let mut run = LabRunBuilder::start(
        LabRunAdmission::Exploratory {
            revision_context: prereg.revision_context.clone(),
            seed_identities: prereg.seed_identities.clone(),
            assumptions: prereg.assumptions.clone(),
            downgrade_reason: DowngradeReason::MissingPersistedPreregistration,
        },
        actual_authority(&prereg),
    )
    .unwrap();

    let completed = run.finalize(ResultEvidence::PreregisteredSeedPass, 30).unwrap();
    assert_eq!(completed.payload.result_class, ResearchResultClass::ExploratoryObservation);
    assert_eq!(completed.payload.preregistration_digest, None);
    assert!(matches!(
        completed.payload.execution_mode,
        ExecutionMode::Exploratory {
            downgrade_reason: DowngradeReason::MissingPersistedPreregistration
        }
    ));
}

#[test]
fn receipt_persisted_at_or_after_start_cannot_authorize_prospective_execution() {
    let prereg = preregistration();
    let receipt = validated_receipt(&prereg, prereg.revision_context.start_sequence);

    assert!(matches!(
        LabRunBuilder::start(
            LabRunAdmission::Prospective {
                preregistration: prereg.clone(),
                receipt,
            },
            actual_authority(&prereg),
        ),
        Err(LabError::PreregistrationNotPersistedBeforeStart {
            receipt_sequence: 20,
            start_sequence: 20,
        })
    ));
}

#[test]
fn prospective_receipt_for_different_preregistration_is_a_hard_error() {
    let prereg = preregistration();
    let mut changed = prereg.clone();
    changed.expected_distinctions.insert("FRESH != STALE".into());
    let receipt = validated_receipt(&changed, 11);

    assert!(matches!(
        LabRunBuilder::start(
            LabRunAdmission::Prospective {
                preregistration: prereg.clone(),
                receipt,
            },
            actual_authority(&prereg),
        ),
        Err(LabError::ProspectiveAuthorityDrift { field: "preregistration_digest" })
    ));
}

#[test]
fn prospective_actual_authority_drift_is_a_hard_error_for_every_bound_field() {
    let prereg = preregistration();

    let mut seed_drift = actual_authority(&prereg);
    seed_drift.seed_catalog_digest = canonical_digest(&json!({"catalog": "other"})).unwrap();
    assert_drift(&prereg, seed_drift, "seed_catalog_digest");

    let mut comparison_drift = actual_authority(&prereg);
    comparison_drift.comparison_profile_revision = "compare-other".into();
    assert_drift(&prereg, comparison_drift, "comparison_profile_revision");

    let mut random_drift = actual_authority(&prereg);
    random_drift.random_source_profile = "rng-other".into();
    assert_drift(&prereg, random_drift, "random_source_profile");

    let mut bound_drift = actual_authority(&prereg);
    bound_drift.model_bound = Some(129);
    assert_drift(&prereg, bound_drift, "model_bound");
}

fn assert_drift(prereg: &LabPreregistration, actual: ActualExecutionAuthority, field: &'static str) {
    assert!(matches!(
        LabRunBuilder::start(prospective_admission(prereg), actual),
        Err(LabError::ProspectiveAuthorityDrift { field: actual_field }) if actual_field == field
    ));
}

#[test]
fn finalized_run_rejects_second_finalize_and_late_observation() {
    let prereg = preregistration();
    let mut run = LabRunBuilder::start(prospective_admission(&prereg), actual_authority(&prereg))
        .unwrap();

    run.finalize(ResultEvidence::PreregisteredSeedPass, 30).unwrap();
    assert!(matches!(
        run.finalize(ResultEvidence::PreregisteredSeedPass, 31),
        Err(LabError::AlreadyFinalized)
    ));
    assert!(matches!(
        run.append_observation_digest(canonical_digest(&"late observation").unwrap()),
        Err(LabError::AlreadyFinalized)
    ));
}
