#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{fs, path::PathBuf, process::Command};

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

        let probe_executable = std::env::var("LOCALVIEW_AX_PERMISSION_PROBE_EXECUTABLE")
            .expect("M02 requires an exact-head fresh-process permission probe executable");
        let provider = AxPermissionProvider::with_dispatch_probe_executable(&probe_executable);

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

        // Prove the independent dispatch fence agrees with the trusted
        // precondition before revocation. This prevents a helper with a
        // different TCC identity from manufacturing a false M02 pass.
        let pre_revoke_dispatch = provider.semantic_dispatch_decision(admitted_permit);
        let pre_revoke_revision = pre_revoke_dispatch.revision();
        assert_eq!(
            pre_revoke_revision.state(),
            AxPermissionState::Trusted,
            "fresh-process dispatch fence must observe the trusted pre-revoke topology"
        );
        let pre_revoke_permit = pre_revoke_dispatch
            .permit()
            .expect("trusted fresh-process fence must refresh semantic authority");
        assert!(
            pre_revoke_permit.permission_check_sequence()
                > admitted_permit.permission_check_sequence(),
            "fresh-process pre-revoke permit must bind a newer provider-owned revision"
        );

        // GitHub-hosted macOS can display a checkbox transition that does not
        // change effective TCC state. Use macOS's own tccutil reset boundary so
        // the seed mutates effective Accessibility authorization rather than
        // treating UI transport as world-state proof.
        reset_accessibility_permission()
            .expect("M02 requires a real mid-session TCC Accessibility revocation");

        // This is the consequential dispatch fence. The already-running test
        // process may still report stale trust, so the provider must derive the
        // decision from a new process. No action is dispatched unless this
        // fresh observation remains trusted.
        let dispatch = provider.semantic_dispatch_decision(pre_revoke_permit);
        let dispatch_revision = dispatch.revision();

        assert_eq!(
            dispatch.admitted_permission_check_sequence(),
            pre_revoke_permit.permission_check_sequence(),
            "dispatch fence must identify the exact authority revision being invalidated"
        );
        assert!(
            dispatch_revision.check_sequence() > pre_revoke_revision.check_sequence(),
            "post-revoke dispatch must bind a newer provider-owned permission observation"
        );
        assert_eq!(
            dispatch_revision.state(),
            AxPermissionState::Untrusted,
            "fresh process must observe the effective TCC Accessibility revoke before dispatch"
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
            "schema": "localview-v43-m02-real-provider-record-v3",
            "case_id": "M02",
            "candidate_sha": candidate_sha,
            "initial_permission_state": "trusted",
            "initial_permission_check_sequence": admitted_revision.check_sequence(),
            "initial_semantic_control_permit_minted": true,
            "fresh_probe_executable": probe_executable,
            "pre_revoke_fresh_probe_state": "trusted",
            "pre_revoke_fresh_probe_check_sequence": pre_revoke_revision.check_sequence(),
            "pre_revoke_refreshed_permit_minted": true,
            "revocation_mechanism": "macos_tccutil_reset_accessibility",
            "tccutil_reset_succeeded": true,
            "dispatch_permission_state": "untrusted",
            "dispatch_permission_check_sequence": dispatch_revision.check_sequence(),
            "admitted_permission_check_sequence": dispatch.admitted_permission_check_sequence(),
            "dispatch_semantic_control_permit_minted": false,
            "typed_permission_revocation": true,
            "denial": "permission_revoked",
            "authority_source": "fresh_process_provider_owned_dispatch_recheck_after_real_tcc_revocation",
        });
        fs::write(
            artifact_dir.join("M02-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M02 record"),
        )
        .expect("write M02 real-provider record");
    }
}
