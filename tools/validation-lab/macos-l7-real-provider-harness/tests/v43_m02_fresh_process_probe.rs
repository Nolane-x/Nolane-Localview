#[cfg(target_os = "macos")]
mod macos_m02_fresh_process_probe {
    use localview_macos_ax_provider::{AxPermissionProvider, AxPermissionState};

    #[test]
    #[ignore = "diagnostic probe for the real macOS Accessibility permission regime"]
    fn m02_fresh_process_permission_state_after_revoke() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "fresh-process M02 probe must be explicitly enabled"
        );

        let provider = AxPermissionProvider::new();
        let revision = provider.current_permission_revision(false);
        eprintln!(
            "M02_FRESH_PROCESS_PERMISSION_STATE={:?} check_sequence={}",
            revision.state(),
            revision.check_sequence(),
        );

        // This diagnostic deliberately does not assert Trusted/Untrusted. Its
        // purpose is to distinguish same-process permission caching from an
        // unsuccessful System Settings revoke in the preceding real-provider
        // oracle. M02's shipping gate remains the exact-head oracle itself.
        let _ = AxPermissionState::Trusted;
    }
}
