#[cfg(windows)]
#[path = "support/v43_weak_accessibility_seed.rs"]
mod weak_accessibility_seed;

#[cfg(windows)]
mod windows_real_provider_w14 {
    use std::{fs, path::PathBuf};

    use localview_protocol::ReconciliationCompleteness;
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaPattern, WindowsUiaPatternSupport,
    };
    use serde_json::json;

    use super::weak_accessibility_seed::{
        WEAK_ACCESSIBILITY_AUTOMATION_ID, WeakAccessibilityEdgeSeedProcess,
        attach_and_snapshot, spawn_worker, truth_str, truth_u64,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    fn w14_owner_drawn_weak_accessibility_never_claims_complete_semantics() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider W14 execution must be explicitly enabled"
        );

        let mut seed = WeakAccessibilityEdgeSeedProcess::spawn();
        let prepared = seed.prepare_weak_accessibility();
        let target_window = truth_u64(&prepared, "window_handle");
        assert_ne!(target_window, 0, "W14 target HWND must be real");
        let visual_label = truth_str(&prepared, "visual_label").to_owned();
        assert_eq!(truth_u64(&prepared, "visual_effect_count"), 0);

        let worker = spawn_worker();
        let (_attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w14:weak-accessibility",
        );

        let (node_index, node) = snapshot
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| {
                node.automation_id.as_deref() == Some(WEAK_ACCESSIBILITY_AUTOMATION_ID)
            })
            .expect("shipping Windows UIA snapshot must retain W14 custom-control shell");
        assert_eq!(
            node.control_type.as_deref(),
            Some("uia_control_type:50025"),
            "W14 seed must surface as the real UIA Custom control type"
        );

        let capabilities = WindowsUiaActionCapabilities::from_node(node);
        assert_eq!(
            capabilities.support_for(WindowsUiaPattern::Invoke),
            WindowsUiaPatternSupport::Unsupported,
            "owner-drawn appearance must not synthesize Invoke capability"
        );
        let semantic_child_count = snapshot
            .nodes()
            .iter()
            .filter(|candidate| candidate.parent_index == Some(node_index))
            .count();
        assert_eq!(
            semantic_child_count, 0,
            "visual-only W14 hotspot must not be forged into a semantic child"
        );

        let serialized_snapshot =
            serde_json::to_string(snapshot.as_ref()).expect("serialize W14 semantic snapshot");
        assert!(
            !serialized_snapshot.contains(&visual_label),
            "visual-only action label must remain visual evidence, not forged UIA semantics"
        );

        assert_eq!(
            node.attributes
                .get("windows_uia.custom_semantic_coverage")
                .map(String::as_str),
            Some("custom_partial"),
            "weak owner-drawn Custom node must explicitly downgrade semantic coverage"
        );
        assert_eq!(
            snapshot.completeness(),
            ReconciliationCompleteness::Incomplete,
            "a custom shell with no semantic children/actions cannot claim complete accessibility"
        );
        assert!(
            snapshot
                .incompleteness_debt()
                .iter()
                .any(|debt| debt == "uia_accessibility_partial_custom_control"),
            "W14 must preserve an explicit accessibility-partial debt for targeted escalation"
        );

        let after = seed.weak_accessibility_state();
        let visual_effect_count = truth_u64(&after, "visual_effect_count");
        assert_eq!(
            visual_effect_count, 0,
            "semantic observation must not click the visual-only owner-drawn hotspot"
        );

        let record = json!({
            "schema": "localview-v43-w14-real-provider-record-v1",
            "candidate_sha": std::env::var("LOCALVIEW_CANDIDATE_SHA").unwrap_or_else(|_| "local-unbound".into()),
            "case_id": "W14",
            "target_automation_id": WEAK_ACCESSIBILITY_AUTOMATION_ID,
            "owner_drawn": true,
            "uia_control_type": "custom",
            "custom_semantic_coverage": "custom_partial",
            "snapshot_completeness": "incomplete",
            "accessibility_partial_debt": true,
            "invoke_support": "unsupported",
            "semantic_child_count": semantic_child_count,
            "visual_label_present_in_semantic_snapshot": false,
            "visual_effect_count": visual_effect_count,
        });
        let serialized_record =
            serde_json::to_string_pretty(&record).expect("serialize W14 evidence record");

        if let Some(dir) = std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR") {
            let dir = PathBuf::from(dir);
            fs::create_dir_all(&dir).expect("create W14 artifact directory");
            fs::write(dir.join("W14-REAL-PROVIDER-RECORD.json"), serialized_record)
                .expect("write W14 real-provider evidence record");
        }

        seed.shutdown();
    }
}
