#[cfg(target_os = "macos")]
mod macos_real_provider_m08 {
    use std::{fs, path::PathBuf};

    use localview_macos_ax_provider::{
        AxPermissionProvider, AxPermissionState, VisualObservationPermissionProvider,
        VisualObservationPermissionState,
    };

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        #[link_name = "CGPreflightScreenCaptureAccess"]
        fn preflight_visual_observation_access() -> bool;
    }

    #[test]
    #[ignore = "requires the real macOS permission regime"]
    fn m08_visual_and_ax_permissions_are_independently_os_observed() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M08 oracle requires LOCALVIEW_CANDIDATE_SHA");
        assert!(!candidate_sha.trim().is_empty());

        let direct_visual_granted = unsafe { preflight_visual_observation_access() };
        let expected_visual_state = if direct_visual_granted {
            VisualObservationPermissionState::Granted
        } else {
            VisualObservationPermissionState::Denied
        };

        let visual_provider = VisualObservationPermissionProvider::new();
        let visual = visual_provider.observation_decision();
        let ax = AxPermissionProvider::new().semantic_control_decision(false);

        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M08 oracle requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M08 artifact directory");

        let visual_state = match visual.revision().state() {
            VisualObservationPermissionState::Granted => "granted",
            VisualObservationPermissionState::Denied => "denied",
            VisualObservationPermissionState::Unknown => "unknown",
        };
        let ax_state = match ax.revision().state() {
            AxPermissionState::Trusted => "trusted",
            AxPermissionState::Untrusted => "untrusted",
            AxPermissionState::Unknown => "unknown",
        };

        let record = serde_json::json!({
            "schema": "localview-v43-m08-real-provider-record-v1",
            "case_id": "M08",
            "candidate_sha": candidate_sha,
            "direct_visual_preflight_granted": direct_visual_granted,
            "visual_permission_state": visual_state,
            "visual_permission_check_sequence": visual.revision().check_sequence(),
            "visual_permit_minted": visual.permit().is_some(),
            "ax_permission_state": ax_state,
            "ax_permission_check_sequence": ax.revision().check_sequence(),
            "ax_semantic_permit_minted": ax.permit().is_some(),
            "visual_authority_source": "coregraphics_preflight",
            "ax_authority_source": "accessibility_provider_live_observation",
            "cross_authority_inference_used": false,
        });
        fs::write(
            artifact_dir.join("M08-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M08 record"),
        )
        .expect("write M08 record");

        assert_eq!(
            visual.revision().state(),
            expected_visual_state,
            "provider visual state must be sourced from the real CoreGraphics preflight, not inferred from AX permission"
        );
        assert_eq!(
            visual.permit().is_some(),
            direct_visual_granted,
            "visual authority must follow its own OS permission only"
        );
        assert!(visual.revision().check_sequence() > 0);
        assert!(ax.revision().check_sequence() > 0);
    }
}
