#[cfg(windows)]
#[path = "support/v43_resource_bounded_seed.rs"]
mod resource_bounded_seed;

#[cfg(windows)]
mod windows_real_provider_w15 {
    use std::{fs, path::PathBuf, time::Duration};

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, SnapshotBudgetLimit};
    use localview_protocol::ReconciliationCompleteness;
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, WindowsObserveRuntimeError,
        spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::WindowsUiaWorkerConfig;
    use serde_json::json;
    use uuid::Uuid;

    use super::resource_bounded_seed::{ResourceBoundedEdgeSeedProcess, truth_u64};

    const W15_MAX_NODES: usize = 1;
    const W15_MAX_DEPTH: usize = 12;
    const W15_MAX_PROPERTIES: usize = 2048;
    const W15_RESOURCE_DEBT: &str = "snapshot_budget_exhausted:Nodes";
    const W14_PARTIAL_DEBT: &str = "uia_accessibility_partial_custom_control";
    const W14_OPAQUE_DEBT: &str = "uia_accessibility_opaque_custom_control";

    #[tokio::test]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w15_low_resource_reconciliation_stays_incomplete_and_typed() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider W15 execution must be explicitly enabled"
        );

        let mut seed = ResourceBoundedEdgeSeedProcess::spawn();
        let truth = seed.ground_truth();
        let target_window = truth_u64(&truth, "window_handle");
        assert_ne!(target_window, 0, "W15 target HWND must be real");
        assert_eq!(
            truth_u64(&truth, "process_id"),
            u64::from(seed.process_id()),
            "W15 oracle process identity must bind the launched WPF seed"
        );

        let bridge = LiveBridge::new(128, 16);
        let manager = spawn_windows_uia_runtime_manager(
            bridge.clone(),
            WindowsUiaWorkerConfig {
                snapshot_budget: SnapshotBudget {
                    max_nodes: W15_MAX_NODES,
                    max_depth: W15_MAX_DEPTH,
                    max_properties: W15_MAX_PROPERTIES,
                },
                command_timeout: Duration::from_secs(5),
            },
            WindowsObserveRuntimeConfig {
                event_capacity: 32,
                drain_limit: 32,
            },
        )
        .expect("spawn production Windows UIA runtime with W15 low-resource budget");

        let session_id = Uuid::new_v4();
        manager
            .attach(session_id, seed.selection(target_window))
            .await
            .expect("attach W15 edge seed through production Windows UIA runtime");

        let initial = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W15 low-resource attach must publish current semantic evidence");
        assert_eq!(
            initial.nodes().len(),
            W15_MAX_NODES,
            "W15 node cap must bound the shipping UIA traversal"
        );
        assert_eq!(
            initial.completeness(),
            ReconciliationCompleteness::Incomplete,
            "budget exhaustion must never be reported as a complete enumeration"
        );
        assert!(initial.resource_usage().incomplete);
        assert_eq!(
            initial.resource_usage().exhausted,
            vec![SnapshotBudgetLimit::Nodes],
            "W15 isolates the node budget as the sole exhausted resource dimension"
        );
        assert!(
            initial
                .incompleteness_debt()
                .iter()
                .any(|debt| debt == W15_RESOURCE_DEBT),
            "W15 initial snapshot must carry explicit node-budget debt"
        );
        assert!(
            initial
                .incompleteness_debt()
                .iter()
                .all(|debt| debt != W14_PARTIAL_DEBT && debt != W14_OPAQUE_DEBT),
            "resource-truncated traversal must not be misdiagnosed as weak custom accessibility"
        );

        let initial_cut = initial.snapshot_cut_ref().to_owned();
        let previous_element_ref = initial
            .nodes()
            .first()
            .expect("W15 bounded snapshot must retain its admitted root node")
            .element_ref
            .clone();

        let error = manager
            .refresh_uia_action_evidence(session_id, previous_element_ref)
            .await
            .expect_err("W15 resource-bounded refresh must never mint fresh action authority");
        assert_eq!(
            error,
            WindowsObserveRuntimeError::ResourceBounded {
                operation: "fresh_action_evidence_snapshot",
                exhausted: vec![SnapshotBudgetLimit::Nodes],
                incompleteness_debt: vec![W15_RESOURCE_DEBT.into()],
            },
            "W15 must expose resource pressure as a typed bounded outcome"
        );

        let fresh = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W15 failed action binding must still retain newest world evidence");
        assert_ne!(
            fresh.snapshot_cut_ref(),
            initial_cut,
            "resource-bounded reconciliation must publish the new observation cut"
        );
        assert_eq!(fresh.completeness(), ReconciliationCompleteness::Incomplete);
        assert!(fresh.resource_usage().incomplete);
        assert_eq!(
            fresh.resource_usage().exhausted,
            vec![SnapshotBudgetLimit::Nodes]
        );
        assert!(
            fresh
                .incompleteness_debt()
                .iter()
                .any(|debt| debt == W15_RESOURCE_DEBT)
        );
        assert!(
            fresh
                .incompleteness_debt()
                .iter()
                .all(|debt| debt != W14_PARTIAL_DEBT && debt != W14_OPAQUE_DEBT)
        );

        let status = manager
            .status(session_id)
            .await
            .expect("W15 runtime status must remain available after bounded reconciliation");
        assert_eq!(
            status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Incomplete)
        );
        assert!(
            status.reconciliation_receipt_id.is_some(),
            "current bounded world evidence must retain its reconciliation receipt"
        );

        let record = json!({
            "schema": "localview-v43-w15-real-provider-record-v1",
            "candidate_sha": std::env::var("LOCALVIEW_CANDIDATE_SHA").unwrap_or_else(|_| "local-unbound".into()),
            "case_id": "W15",
            "budget": {
                "max_nodes": W15_MAX_NODES,
                "max_depth": W15_MAX_DEPTH,
                "max_properties": W15_MAX_PROPERTIES,
            },
            "initial_nodes_observed": initial.nodes().len(),
            "initial_snapshot_completeness": "incomplete",
            "fresh_snapshot_completeness": "incomplete",
            "nodes_budget_exhausted": true,
            "resource_debt_present": true,
            "resource_error_typed": true,
            "fresh_cut_published": true,
            "reconciliation_receipt_present": status.reconciliation_receipt_id.is_some(),
            "action_binding_minted": false,
            "accessibility_partial_debt_present": false,
        });
        let serialized_record =
            serde_json::to_string_pretty(&record).expect("serialize W15 evidence record");
        if let Some(dir) = std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR") {
            let dir = PathBuf::from(dir);
            fs::create_dir_all(&dir).expect("create W15 artifact directory");
            fs::write(dir.join("W15-REAL-PROVIDER-RECORD.json"), serialized_record)
                .expect("write W15 real-provider evidence record");
        }

        manager
            .release(session_id)
            .await
            .expect("release W15 Windows UIA runtime");
        assert!(
            bridge.observation_status(session_id).await.is_none(),
            "W15 release must clean the runtime observation state"
        );
        seed.shutdown();
    }
}
