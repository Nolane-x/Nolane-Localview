#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;
#[cfg(windows)]
#[path = "support/v43_geometry_cases.rs"]
mod geometry_cases;
#[cfg(windows)]
#[path = "support/v43_w10_capability.rs"]
mod w10_capability;

#[cfg(windows)]
mod windows_real_provider_w10 {
    use localview_validation_lab::ResultEvidence;

    use super::{geometry_cases, w10_capability};

    #[test]
    fn w10_hosted_capability_probe_writes_explicit_non_closure_artifact() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let capability = w10_capability::probe_and_write();
        assert_eq!(capability.case_id, "W10-mixed-dpi-window-movement");
        assert!(!capability.mints_real_provider_integration_pass);
        assert!(capability.monitor_count >= 1);
        assert!(!capability.effective_dpi_values.is_empty());
        if !capability.measured {
            assert!(
                capability.reason.as_deref().is_some_and(|reason| !reason.is_empty()),
                "an unmeasured W10 capability record must explain why closure was not measured"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and two displays with distinct effective DPI"]
    async fn w10_mixed_dpi_move_refreshes_live_physical_geometry_without_stale_coordinates() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let record = geometry_cases::run_w10(
            "focused-w10-edge-seed",
            "focused-w10-physical-environment",
            "windows-uia-physical-mixed-dpi-r1",
            "real-provider-exact-r1",
            110,
        )
        .await;
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass),
            "W10 exact physical oracle must produce integration-pass evidence only when both geometry observations match the independent oracle"
        );
        assert!(record.observation.failure_flags.is_empty());
        assert!(record.observation.provider_backed);
    }
}
