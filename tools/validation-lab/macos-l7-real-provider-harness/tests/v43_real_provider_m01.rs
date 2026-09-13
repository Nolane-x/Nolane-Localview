#[cfg(target_os = "macos")]
mod macos_real_provider_m01 {
    use std::{fs, path::PathBuf};

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

    #[test]
    #[ignore = "requires the real macOS Accessibility permission regime"]
    fn m01_accessibility_permission_absent_is_typed_and_non_authorizing() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );
        assert_eq!(
            std::env::var("LOCALVIEW_M01_TCC_RESET").as_deref(),
            Ok("1"),
            "M01 oracle requires a successful Accessibility TCC reset before observation"
        );

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("real-provider M01 oracle requires LOCALVIEW_CANDIDATE_SHA");
        assert!(
            !candidate_sha.trim().is_empty(),
            "candidate SHA must not be empty"
        );

        let provider = AxPermissionProvider::new();
        let decision = provider.semantic_control_decision(false);
        let revision = decision.revision();

        assert_eq!(
            revision.state(),
            AxPermissionState::Untrusted,
            "M01 requires a real untrusted Accessibility topology after TCC reset; if this runner stays trusted, do not fabricate an absent-permission pass"
        );
        assert!(
            !revision.prompt_requested(),
            "M01 must observe absent permission without displaying or assuming a prompt"
        );
        assert!(
            revision.check_sequence() > 0,
            "real permission observation must carry a revision sequence"
        );
        assert_eq!(
            decision.outcome(),
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRequired),
            "the exact untrusted OS-backed revision must produce typed permission denial"
        );
        assert_eq!(
            decision.permit(),
            None,
            "untrusted AX permission must not mint semantic-control authority"
        );

        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("real-provider M01 oracle requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M01 artifact directory");
        let record = serde_json::json!({
            "schema": "localview-v43-m01-real-provider-record-v1",
            "case_id": "M01",
            "candidate_sha": candidate_sha,
            "tcc_reset_applied": true,
            "permission_state": "untrusted",
            "permission_check_sequence": revision.check_sequence(),
            "prompt_requested": revision.prompt_requested(),
            "semantic_control_permit_minted": false,
            "typed_permission_denial": true,
            "denial": "permission_required",
            "authority_source": "provider_owned_os_observation",
        });
        fs::write(
            artifact_dir.join("M01-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M01 record"),
        )
        .expect("write M01 real-provider record");
    }
}
