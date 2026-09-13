#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{fs, path::PathBuf, process::Command};

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

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
            "M02 requires a real initially trusted Accessibility topology; if the runner is already untrusted, do not fabricate a revocation pass"
        );
        let admitted_permit = admission
            .permit()
            .expect("trusted M02 precondition must mint semantic-control admission authority");
        assert_eq!(
            admitted_permit.permission_check_sequence(),
            admitted_revision.check_sequence(),
            "admitted permit must bind the exact initially trusted revision"
        );

        let reset = Command::new("sudo")
            .args(["/usr/bin/tccutil", "reset", "Accessibility"])
            .status()
            .expect("invoke real Accessibility TCC reset inside the same M02 process");
        assert!(
            reset.success(),
            "M02 requires a successful real Accessibility TCC reset after admission"
        );

        let dispatch = provider.semantic_dispatch_decision(admitted_permit);
        let dispatch_revision = dispatch.revision();

        assert_eq!(
            dispatch.admitted_permission_check_sequence(),
            admitted_permit.permission_check_sequence(),
            "dispatch fence must identify the exact authority revision being invalidated"
        );
        assert!(
            dispatch_revision.check_sequence() > admitted_revision.check_sequence(),
            "dispatch must take a newer provider-owned OS permission observation"
        );
        assert_eq!(
            dispatch_revision.state(),
            AxPermissionState::Untrusted,
            "real TCC reset must be observed as revoked before dispatch"
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
            "schema": "localview-v43-m02-real-provider-record-v1",
            "case_id": "M02",
            "candidate_sha": candidate_sha,
            "initial_permission_state": "trusted",
            "initial_permission_check_sequence": admitted_revision.check_sequence(),
            "initial_semantic_control_permit_minted": true,
            "tcc_reset_applied_mid_session": true,
            "dispatch_permission_state": "untrusted",
            "dispatch_permission_check_sequence": dispatch_revision.check_sequence(),
            "admitted_permission_check_sequence": dispatch.admitted_permission_check_sequence(),
            "dispatch_semantic_control_permit_minted": false,
            "typed_permission_revocation": true,
            "denial": "permission_revoked",
            "authority_source": "provider_owned_dispatch_recheck",
        });
        fs::write(
            artifact_dir.join("M02-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M02 record"),
        )
        .expect("write M02 real-provider record");
    }
}
