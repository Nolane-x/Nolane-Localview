#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{fs, path::PathBuf, process::Command};

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

    const ACCESSIBILITY_DEEP_LINK: &str =
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";

    const TOGGLE_BASH_ACCESSIBILITY_OFF_JXA: &str = r#"
        const host = Application.currentApplication();
        host.includeStandardAdditions = true;
        const se = Application('System Events');
        const proc = se.processes.byName('System Settings');
        proc.frontmost = true;

        function safe(fn) {
          try { return fn(); } catch (_) { return null; }
        }

        let win = null;
        for (let attempt = 0; attempt < 100; attempt++) {
          const wins = safe(() => proc.windows()) || [];
          if (wins.length > 0 && String(safe(() => wins[0].name()) || '') === 'Accessibility') {
            win = wins[0];
            break;
          }
          host.delay(0.1);
        }
        if (win === null) throw new Error('Accessibility privacy pane did not become ready');

        function findBashSwitch(node, depth) {
          if (depth > 16) return null;
          const role = String(safe(() => node.role()) || '');
          const name = String(safe(() => node.name()) || '');
          if (role === 'AXCheckBox' && name === 'bash') return node;

          const children = safe(() => node.uiElements()) || [];
          for (const child of children) {
            const found = findBashSwitch(child, depth + 1);
            if (found !== null) return found;
          }
          return null;
        }

        const bashSwitch = findBashSwitch(win, 0);
        if (bashSwitch === null) throw new Error('bash Accessibility switch not found in ready Accessibility pane');

        const before = Number(safe(() => bashSwitch.value()));
        if (before !== 1) throw new Error(`bash Accessibility switch precondition expected ON, got ${before}`);

        const actions = safe(() => bashSwitch.actions()) || [];
        const actionNames = actions.map(action => String(safe(() => action.name()) || ''));
        const byNameExists = Boolean(safe(() => bashSwitch.actions.byName('AXPress').exists()));
        console.log(
          `M02_ACTION_DIAGNOSTIC role=${String(safe(() => bashSwitch.role()) || '')} name=${String(safe(() => bashSwitch.name()) || '')} before=${before} actions=${JSON.stringify(actionNames)} axpress_by_name_exists=${byNameExists}`
        );
        throw new Error('M02_ACTION_DIAGNOSTIC_COMPLETE');
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
            .args(["-l", "JavaScript", "-e", TOGGLE_BASH_ACCESSIBILITY_OFF_JXA])
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
