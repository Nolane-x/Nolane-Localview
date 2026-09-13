#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{fs, path::PathBuf, process::Command};

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

    const ACCESSIBILITY_DEEP_LINK: &str =
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";

    const TOGGLE_BASH_ACCESSIBILITY_OFF: &str = r#"
        tell application "System Settings" to activate
        tell application "System Events"
          tell process "System Settings"
            set frontmost to true

            set paneReady to false
            repeat with attempt from 1 to 100
              if exists window 1 then
                try
                  if name of window 1 is "Accessibility" then
                    set paneReady to true
                    exit repeat
                  end if
                end try
              end if
              delay 0.1
            end repeat
            if paneReady is false then error "Accessibility privacy pane did not become ready"

            set foundSwitch to false
            set allItems to entire contents of window 1
            repeat with itemRef in allItems
              try
                if role of itemRef is "AXCheckBox" and name of itemRef is "bash" then
                  if value of itemRef is 1 then click itemRef
                  set foundSwitch to true
                  exit repeat
                end if
              end try
            end repeat
            if foundSwitch is false then error "bash Accessibility switch not found in ready Accessibility pane"
          end tell
        end tell
    "#;

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

        let open_settings = Command::new("/usr/bin/open")
            .arg(ACCESSIBILITY_DEEP_LINK)
            .status()
            .expect("open real Privacy & Security > Accessibility pane");
        assert!(
            open_settings.success(),
            "M02 requires the real Accessibility privacy pane to open"
        );

        let toggle = Command::new("/usr/bin/osascript")
            .args(["-e", TOGGLE_BASH_ACCESSIBILITY_OFF])
            .status()
            .expect("toggle the real bash Accessibility switch off through System Settings UI");
        assert!(
            toggle.success(),
            "M02 requires a user-equivalent mid-session Accessibility revoke of the responsible bash process"
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
            "the real System Settings Accessibility revoke must be observed before dispatch"
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
            "system_settings_accessibility_pane_opened": true,
            "responsible_process_switch": "bash",
            "system_settings_bash_switch_toggled_off_mid_session": true,
            "dispatch_permission_state": "untrusted",
            "dispatch_permission_check_sequence": dispatch_revision.check_sequence(),
            "admitted_permission_check_sequence": dispatch.admitted_permission_check_sequence(),
            "dispatch_semantic_control_permit_minted": false,
            "typed_permission_revocation": true,
            "denial": "permission_revoked",
            "authority_source": "provider_owned_dispatch_recheck_after_user_equivalent_system_settings_revoke",
        });
        fs::write(
            artifact_dir.join("M02-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M02 record"),
        )
        .expect("write M02 real-provider record");
    }
}
