#[cfg(windows)]
#[path = "support/v43_baseline_cases.rs"]
mod baseline_cases;
#[cfg(windows)]
#[path = "support/v43_campaign_w11_w12_support.rs"]
mod campaign_w11_w12_support;
#[cfg(windows)]
#[path = "support/v43_campaign_w10_w12_records.rs"]
mod campaign_records;
#[cfg(windows)]
#[path = "support/v43_campaign_w10_w12_run.rs"]
mod campaign_run;
#[cfg(windows)]
#[path = "support/v43_campaign_w10_w12_support.rs"]
mod campaign_support;
#[cfg(windows)]
#[path = "support/v43_follow_on_cases.rs"]
mod follow_on_cases;
#[cfg(windows)]
#[path = "support/v43_geometry_cases.rs"]
mod geometry_cases;
#[cfg(windows)]
#[path = "support/v43_verified_input_cases.rs"]
mod verified_input_cases;
#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;
#[cfg(windows)]
#[path = "support/v43_verified_input_w11_w12_cases.rs"]
mod verified_input_w11_w12_cases;

#[cfg(windows)]
mod windows_l7_real_provider_campaign_w10_w12 {
    use super::campaign_run;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires the physical Windows mixed-DPI L7 lab with all W01-W12 provider seeds"]
    async fn prospective_l7_campaign_binds_w01_w12_to_exact_candidate() {
        campaign_run::run_exact_campaign().await;
    }
}
