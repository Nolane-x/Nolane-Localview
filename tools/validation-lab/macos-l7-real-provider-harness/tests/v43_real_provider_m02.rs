#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{fs, path::PathBuf, process::Command, thread, time::Duration};

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

    const TCCUTIL_PATH: &str = "/usr/bin/tccutil";

    fn reset_accessibility_permission() -> Result<(), String> {
        let output = Command::new(TCCUTIL_PATH)
            .args(["reset", "Accessibility"])
            .output()
            .map_err(|error| format!("launch tccutil reset Accessibility: {error}"))?;

        if !output.status.success() {
            return Err(format!(
                "tccutil reset Accessibility failed: status={} stdout={:?} stderr={:?}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            ));
        }

        eprintln!(
            "M02_TCCUTIL_RESET status=success stdout={:?}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
        Ok(())
    }

    fn wait_for_effective_revocation(
        provider: &AxPermissionProvider,
    ) -> Result<u64, String> {
        let mut last_state = AxPermissionState::Unknown;
        let mut last_sequence = 0;

        for _ in 0..50 {
            let revision = provider.current_permission_revision(false);
            last_state = revision.state();
            last_sequence = revision.check_sequence();
            if revision.state() == AxPermissionState::Untrusted {
                eprintln!(
                    "M02_EFFECTIVE_REVOCATION state=Untrusted check_sequence={}",
                    revision.check_sequence()
                );
                return Ok(revision.check_sequence());
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(format!(
            "effective Accessibility revocation was not observed within 5s; last_state={last_state:?} last_sequence={last_sequence}"
        ))
    }

    #[test]
    #[ignore = "requires the real macOS Accessibility permission regime"]
    fn m02_permission_revoked_mid_session_invalidates_prior_semantic_authority() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("real-provider M02 oracle requires LOCALVIEW_CANDIDATE_SHA");
        assert!(
            !candidate_sha.trim().is_empty(),
            "candidate SHA must not be empty"
        );

        let provider = AxPermissionProvider::new();
        let admission = provider.semantic_control_decision(false);
        let admitted_revision = admission.revision();

        assert_eq!(
            admitted_revision.state(),
            AxPermissionState::Trusted,
            "M02 requires a real initially trusted Accessibility topology; an untrusted runner cannot fabricate a revocation pass"
        );
        let admitted_permit = admission
            .permit()
            .expect("trusted M02 precondition must mint semantic-control admission authority");
        assert_eq!(
            admitted_permit.permission_check_sequence(),
            admitted_revision.check_sequence(),
            "admitted permit must bind the exact initially trusted revision"
        );

        // The GitHub-hosted macOS image exposes a pre-authorized Accessibility
        // fixture. Its System Settings checkbox can visually flip OFF without
        // changing the effective TCC authorization. For the real-provider M02
        // oracle, use macOS's own test/reset boundary instead: this changes the
        // actual TCC authorization observed by AXIsProcessTrusted while the
        // LocalView test process remains alive.
        reset_accessibility_permission()
            .expect("M02 requires a real mid-session TCC Accessibility revocation");
        let revocation_probe_sequence = wait_for_effective_revocation(&provider)
            .expect("M02 must observe the OS revocation before dispatch");

        let dispatch = provider.semantic_dispatch_decision(admitted_permit);
        let dispatch_revision = dispatch.revision();

        assert_eq!(
            dispatch.admitted_permission_check_sequence(),
            admitted_permit.permission_check_sequence(),
            "dispatch fence must identify the exact authority revision being invalidated"
        );
        assert!(
            dispatch_revision.check_sequence() > revocation_probe_sequence,
            "dispatch must take a provider-owned permission observation newer than the observed revoke"
        );
        assert_eq!(
            dispatch_revision.state(),
            AxPermissionState::Untrusted,
            "the effective TCC Accessibility revoke must still be observed at dispatch"
        );
        assert_eq!(
            dispatch.outcome(),
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRevoked),
            "revoked permission must invalidate prior semantic-control authority with a typed denial"
        );
        assert_eq!(
            dispatch.permit(),
            None,
            "revoked dispatch must not mint replacement semantic-control authority"
        );

        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("real-provider M02 oracle requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M02 artifact directory");
        let record = serde_json::json!({
            "schema": "localview-v43-m02-real-provider-record-v2",
            "case_id": "M02",
            "candidate_sha": candidate_sha,
            "initial_permission_state": "trusted",
            "initial_permission_check_sequence": admitted_revision.check_sequence(),
            "initial_semantic_control_permit_minted": true,
            "revocation_mechanism": "macos_tccutil_reset_accessibility",
            "tccutil_reset_succeeded": true,
            "effective_revocation_observed": true,
            "effective_revocation_probe_sequence": revocation_probe_sequence,
            "dispatch_permission_state": "untrusted",
            "dispatch_permission_check_sequence": dispatch_revision.check_sequence(),
            "admitted_permission_check_sequence": dispatch.admitted_permission_check_sequence(),
            "dispatch_semantic_control_permit_minted": false,
            "typed_permission_revocation": true,
            "denial": "permission_revoked",
            "authority_source": "provider_owned_dispatch_recheck_after_real_tcc_revocation",
        });
        fs::write(
            artifact_dir.join("M02-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M02 record"),
        )
        .expect("write M02 real-provider record");
    }
}
