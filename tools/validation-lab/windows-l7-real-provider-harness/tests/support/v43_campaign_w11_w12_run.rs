use std::fs;

use localview_validation_lab::{
    ActualExecutionAuthority, CampaignLayer, LabMetricKind, LabPreregistration,
    LabRevisionContext, LabRunAdmission, LabRunBuilder, PersistedPreregistrationReceipt,
    ProviderCampaignKind, ResearchResultClass, ResultEvidence, canonical_digest,
    derive_real_provider_campaign_evidence, validate_persisted_receipt,
};
use serde_json::json;

use super::{campaign_records, campaign_support as support};

pub async fn run_exact_campaign() {
    assert!(std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some());
    let candidate_sha = support::required_env("LOCALVIEW_CANDIDATE_SHA");
    let classic_seed_digest = support::executable_digest("LOCALVIEW_UIA_SEED_BIN");
    let edge_seed_digest = support::executable_digest("LOCALVIEW_UIA_EDGE_SEED_BIN");
    let artifact_dir = support::artifact_dir();
    let environment = support::environment_manifest(
        &candidate_sha,
        &classic_seed_digest,
        &edge_seed_digest,
    );
    support::write_json(&artifact_dir.join("environment-manifest.json"), &environment);
    let environment_digest =
        canonical_digest(&environment).expect("digest eleven-seed environment manifest");
    let seed_catalog_digest = canonical_digest(&json!({
        "candidate_sha": candidate_sha,
        "environment_digest": environment_digest.0.clone(),
        "classic_seed_executable_digest": classic_seed_digest.clone(),
        "edge_seed_executable_digest": edge_seed_digest.clone(),
        "required_cases": support::REQUIRED_CASES,
    }))
    .expect("digest exact eleven-seed catalog");

    let preregistration = LabPreregistration {
        revision_context: LabRevisionContext {
            lab_revision: "lab-v43-windows-l7-r4".into(),
            seed_corpus_revision: "windows-provider-seeds-w01-w09-w11-w12-r4".into(),
            spec_revision_digest: "v4.3-principal-provider-reconciliation-closure".into(),
            reference_reducer_revision: "provider-oracle-r1".into(),
            mutation_catalog_revision: "windows-provider-seed-matrix-r1".into(),
            comparison_profile_revision: support::COMPARISON_PROFILE.into(),
            random_source_profile: support::RANDOM_SOURCE_PROFILE.into(),
            platform_profile: Some(support::PLATFORM_PROFILE.into()),
            start_sequence: support::CAMPAIGN_START_SEQUENCE,
        },
        seed_catalog_digest: seed_catalog_digest.clone(),
        seed_identities: support::seed_identities(),
        campaign_layer: CampaignLayer::L7,
        expected_distinctions: support::expected_distinctions(),
        model_bound: None,
        assumptions: support::assumptions(),
        declared_metrics: support::declared_metrics(),
        creation_sequence: 80,
    };
    let prepared = preregistration
        .prepare()
        .expect("prepare eleven-seed L7 preregistration");
    let prereg_path = artifact_dir.join("LAB-PREREGISTRATION.json");
    fs::write(&prereg_path, &prepared.canonical_bytes)
        .expect("persist preregistration before campaign start");
    let persisted_receipt = PersistedPreregistrationReceipt {
        digest: prepared.digest.clone(),
        logical_sequence: 90,
        persistence_ref: prereg_path.to_string_lossy().into_owned(),
    };
    support::write_json(
        &artifact_dir.join("LAB-PREREGISTRATION-RECEIPT.json"),
        &persisted_receipt,
    );
    let receipt = validate_persisted_receipt(&prepared, persisted_receipt)
        .expect("validate persisted preregistration receipt");
    let authority = ActualExecutionAuthority {
        seed_catalog_digest,
        comparison_profile_revision: support::COMPARISON_PROFILE.into(),
        random_source_profile: support::RANDOM_SOURCE_PROFILE.into(),
        model_bound: None,
    };
    let admission = LabRunAdmission::Prospective {
        preregistration: preregistration.clone(),
        receipt,
    };
    let mut run = LabRunBuilder::start_provider_campaign(
        ProviderCampaignKind::RealProviderSeedApplications,
        admission,
        authority,
    )
    .expect("start typed prospective eleven-seed campaign");

    let records = campaign_records::execute_records(
        &classic_seed_digest,
        &edge_seed_digest,
        &environment_digest.0,
    )
    .await;
    campaign_records::validate_records(&records);
    support::write_json(&artifact_dir.join("W11-REAL-PROVIDER-RECORD.json"), &records[9]);
    support::write_json(&artifact_dir.join("W12-REAL-PROVIDER-RECORD.json"), &records[10]);
    for record in &records {
        run.append_observation(record.observation.clone())
            .expect("append exact provider observation");
    }
    let evidence = derive_real_provider_campaign_evidence(&records)
        .expect("derive eleven-record provider evidence");
    assert_eq!(evidence, ResultEvidence::RealProviderIntegrationPass);
    let completed = run
        .finalize(evidence, 112)
        .expect("finalize exact eleven-case prospective campaign");
    let rpomr = completed
        .payload
        .metric_snapshot
        .get(LabMetricKind::Rpomr)
        .expect("RPOMR metric");
    assert_eq!((rpomr.numerator, rpomr.denominator), (0, 11));
    assert_eq!(completed.payload.observation_digests.len(), 11);
    assert_eq!(completed.payload.result_class, ResearchResultClass::RealProviderIntegrationPass);
    assert_eq!(
        completed.payload.actual_execution_authority.seed_catalog_digest,
        preregistration.seed_catalog_digest
    );
    support::write_json(&artifact_dir.join("LAB-RESULT.json"), &completed);
}
