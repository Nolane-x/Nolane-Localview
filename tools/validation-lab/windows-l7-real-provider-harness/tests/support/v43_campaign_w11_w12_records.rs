use std::collections::BTreeSet;

use localview_validation_lab::{RealProviderLabRecord, ResultEvidence};

use super::{
    baseline_cases, campaign_support as support, follow_on_cases, verified_input_cases,
    verified_input_w11_w12_cases,
};

pub async fn execute_records(
    classic_seed_digest: &str,
    edge_seed_digest: &str,
    environment_digest: &str,
) -> Vec<RealProviderLabRecord> {
    vec![
        baseline_cases::run_w01(classic_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 101).await,
        baseline_cases::run_w02(classic_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 102).await,
        follow_on_cases::run_w03(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 103).await,
        follow_on_cases::run_w04(classic_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 104),
        follow_on_cases::run_w05(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 105),
        baseline_cases::run_w06(classic_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 106).await,
        verified_input_cases::run_w07(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 107).await,
        verified_input_cases::run_w08(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 108).await,
        verified_input_cases::run_w09(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 109).await,
        verified_input_w11_w12_cases::run_w11(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 110).await,
        verified_input_w11_w12_cases::run_w12(edge_seed_digest, environment_digest, support::PLATFORM_PROFILE, support::COMPARISON_PROFILE, 111).await,
    ]
}

pub fn validate_records(records: &[RealProviderLabRecord]) {
    assert_eq!(records.len(), 11, "exact campaign must contain eleven records");
    let observed_seed_ids = records
        .iter()
        .map(|record| record.observation.seed_id.clone().expect("seed id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        observed_seed_ids,
        support::REQUIRED_CASES
            .iter()
            .map(|seed| (*seed).to_owned())
            .collect::<BTreeSet<_>>(),
        "exact set must be W01-W09 plus W11-W12; W10 is unmeasured"
    );
    for record in records {
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        assert!(record.observation.provider_backed);
        assert!(record.observation.failure_flags.is_empty());
    }
}
