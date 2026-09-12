#[cfg(windows)]
mod windows_real_provider_w03 {
    use std::{
        collections::BTreeSet,
        fs,
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::ProviderElementRealization;
    use localview_validation_lab::{
        LabMetricKind, RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth,
        RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case, canonical_digest,
    };
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::{
        WindowsUiaItemLookupProperty, WindowsUiaVirtualizedItemQueryRequest,
        WindowsUiaWorkerConfig,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    const W03_CONTAINER_AUTOMATION_ID: &str = "LocalViewW03VirtualizedItems";
    const W03_VIRTUAL_ITEM_NAME: &str = "LocalView Virtual Item 255";
    const PLATFORM_PROFILE: &str = "windows-uia-hosted-r1";
    const COMPARISON_PROFILE: &str = "real-provider-exact-r1";

    struct EdgeSeedProcess {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        shutdown: bool,
    }

    impl EdgeSeedProcess {
        fn spawn() -> Self {
            let binary = std::env::var_os("LOCALVIEW_UIA_EDGE_SEED_BIN")
                .expect("LOCALVIEW_UIA_EDGE_SEED_BIN must point to the WPF edge seed executable");
            let mut child = Command::new(binary)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch isolated WPF UIA edge seed process");
            let stdin = child.stdin.take().expect("edge seed stdin must be piped");
            let stdout = child.stdout.take().expect("edge seed stdout must be piped");
            Self {
                child,
                stdin,
                stdout: BufReader::new(stdout),
                shutdown: false,
            }
        }

        fn process_id(&self) -> u32 {
            self.child.id()
        }

        fn command(&mut self, command: Value) -> Value {
            serde_json::to_writer(&mut self.stdin, &command).expect("serialize edge seed command");
            writeln!(self.stdin).expect("terminate edge seed command JSON line");
            self.stdin.flush().expect("flush edge seed command");
            let mut line = String::new();
            self.stdout
                .read_line(&mut line)
                .expect("read edge seed JSON-line response");
            assert!(
                !line.trim().is_empty(),
                "edge seed closed its oracle channel unexpectedly"
            );
            serde_json::from_str(&line).expect("parse edge seed JSON-line response")
        }

        fn ground_truth(&mut self) -> Value {
            let response = self.command(json!({ "command": "get_ground_truth" }));
            assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
            response
        }

        fn virtual_item_generated(&mut self) -> bool {
            let response = self.command(json!({ "command": "get_virtual_item_state" }));
            assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
            response
                .get("virtual_item_container_generated")
                .and_then(Value::as_bool)
                .expect("edge oracle must report virtual_item_container_generated")
        }

        fn wait_until_virtual_item_generated(&mut self, timeout: Duration) {
            let deadline = Instant::now() + timeout;
            loop {
                if self.virtual_item_generated() {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "W03 provider Realize returned but the independent WPF oracle never observed a generated item container"
                );
                thread::sleep(Duration::from_millis(25));
            }
        }

        fn shutdown(mut self) {
            let response = self.command(json!({ "command": "shutdown" }));
            assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
            self.shutdown = true;
            let status = self.child.wait().expect("wait for WPF edge seed shutdown");
            assert!(status.success(), "WPF edge seed must exit cleanly: {status}");
        }
    }

    impl Drop for EdgeSeedProcess {
        fn drop(&mut self) {
            if !self.shutdown {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    fn truth_u64(truth: &Value, field: &str) -> u64 {
        truth
            .get(field)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("edge ground-truth field {field} must be u64"))
    }

    fn edge_seed_digest() -> String {
        let path = std::env::var_os("LOCALVIEW_UIA_EDGE_SEED_BIN")
            .expect("LOCALVIEW_UIA_EDGE_SEED_BIN must be bound for digest authority");
        let bytes = fs::read(path).expect("read exact WPF edge seed executable for digest binding");
        let digest = Sha256::digest(bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w03_virtualized_item_requires_realize_then_fresh_observation() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let initial_truth = seed.ground_truth();
        assert_eq!(
            initial_truth.get("virtual_item_name").and_then(Value::as_str),
            Some(W03_VIRTUAL_ITEM_NAME)
        );
        assert_eq!(
            initial_truth
                .get("virtual_item_container_generated")
                .and_then(Value::as_bool),
            Some(false),
            "W03 must begin with the tail item genuinely virtualized"
        );
        assert_eq!(
            truth_u64(&initial_truth, "process_id"),
            u64::from(seed.process_id()),
            "independent oracle process identity must match the launched seed"
        );
        let window_handle = truth_u64(&initial_truth, "window_handle");
        assert_ne!(window_handle, 0, "W03 oracle must expose a live WPF HWND");

        let bridge = LiveBridge::new(128, 16);
        let manager = spawn_windows_uia_runtime_manager(
            bridge.clone(),
            WindowsUiaWorkerConfig {
                snapshot_budget: SnapshotBudget {
                    max_nodes: 128,
                    max_depth: 12,
                    max_properties: 2048,
                },
                command_timeout: Duration::from_secs(5),
            },
            WindowsObserveRuntimeConfig {
                event_capacity: 32,
                drain_limit: 32,
            },
        )
        .expect("spawn production Windows UIA runtime for W03");
        let session_id = Uuid::new_v4();
        manager
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: seed.process_id(),
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach exact W03 WPF seed window through production runtime");

        let before = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("runtime must publish W03 pre-realization current snapshot");
        let container = before
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(W03_CONTAINER_AUTOMATION_ID))
            .expect("real UIA runtime snapshot must expose the deterministic W03 ItemContainer");
        assert_eq!(
            container.element_ref.realization,
            ProviderElementRealization::RealizedCurrent
        );
        assert!(
            !seed.virtual_item_generated(),
            "initial runtime snapshot must not realize the target WPF item"
        );

        let query = WindowsUiaVirtualizedItemQueryRequest::new(
            before.snapshot_cut_ref(),
            container.element_ref.clone(),
            WindowsUiaItemLookupProperty::Name,
            W03_VIRTUAL_ITEM_NAME,
        )
        .expect("construct exact W03 ItemContainer query from current runtime cut");
        let receipt = manager
            .realize_virtualized_item_and_refresh(session_id, query)
            .await
            .expect("runtime must serialize ItemContainer lookup, Realize, and fresh reconciliation");

        assert_eq!(
            receipt.query_receipt.placeholder_element_ref.realization,
            ProviderElementRealization::RealizationRequired,
            "provider-virtualized placeholder must remain explicitly non-actionable"
        );
        assert_eq!(
            receipt.realize_receipt.previous_placeholder_ref,
            receipt.query_receipt.placeholder_element_ref,
            "realization receipt must preserve stale placeholder identity rather than upgrade it"
        );
        assert_ne!(receipt.fresh_snapshot.snapshot_cut_ref(), before.snapshot_cut_ref());
        assert!(
            receipt
                .fresh_snapshot
                .nodes()
                .iter()
                .all(|node| node.element_ref != receipt.query_receipt.placeholder_element_ref),
            "old provider placeholder must never survive into fresh current runtime state"
        );

        seed.wait_until_virtual_item_generated(Duration::from_secs(2));
        let realized = receipt
            .fresh_snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(W03_VIRTUAL_ITEM_NAME))
            .expect("fresh real-provider runtime snapshot must contain the realized W03 item");
        assert_eq!(realized.element_ref.realization, ProviderElementRealization::RealizedCurrent);
        assert_eq!(
            realized.element_ref.acquisition_cut_ref,
            receipt.fresh_snapshot.snapshot_cut_ref()
        );
        assert_ne!(
            realized.element_ref.acquisition_cut_ref,
            receipt.realize_receipt.previous_placeholder_ref.acquisition_cut_ref,
            "old placeholder cut must never become fresh action authority"
        );

        let current = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("fresh post-realization snapshot must become runtime current state");
        assert_eq!(current.observed_digest(), receipt.fresh_snapshot.observed_digest());

        let final_truth = seed.ground_truth();
        assert_eq!(
            final_truth
                .get("virtual_item_container_generated")
                .and_then(Value::as_bool),
            Some(true),
            "independent WPF oracle must prove the target item is physically generated"
        );
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .unwrap_or_else(|_| "unknown:standalone-w03-candidate".into());
        let seed_digest = edge_seed_digest();
        let environment_digest = canonical_digest(&json!({
            "candidate_sha": candidate_sha,
            "platform_profile": PLATFORM_PROFILE,
            "architecture": std::env::consts::ARCH,
            "seed_digest": seed_digest,
        }))
        .expect("digest standalone W03 hosted environment");
        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W03-virtualized-item-realization",
            seed_app_digest: &seed_digest,
            platform_profile_revision: PLATFORM_PROFILE,
            environment_artifact_digest: &environment_digest.0,
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!(
                    "windows-uia:after:{}",
                    receipt.fresh_snapshot.observed_digest()
                ),
                format!("windows-uia:reconciliation:{}", receipt.reconciliation_receipt_ref),
                format!(
                    "windows-uia:placeholder-cut:{}",
                    receipt.query_receipt.snapshot_cut_ref
                ),
            ]),
            ground_truth: RealProviderGroundTruth {
                canonical_outcome: "virtual-item-realized-under-fresh-cut".into(),
                digest: canonical_digest(&final_truth).expect("digest W03 independent WPF oracle truth"),
            },
            observed_outcome: RealProviderObservedOutcome::Asserted(
                "virtual-item-realized-under-fresh-cut".into(),
            ),
            case_kind: RealProviderCaseKind::W03VirtualizedItemRealization {
                placeholder_blocked_before_realization: receipt
                    .query_receipt
                    .placeholder_element_ref
                    .realization
                    == ProviderElementRealization::RealizationRequired,
                fresh_cut_after_realization: receipt.fresh_snapshot.snapshot_cut_ref()
                    != before.snapshot_cut_ref(),
                realized_current_after_fresh_cut: realized.element_ref.realization
                    == ProviderElementRealization::RealizedCurrent
                    && realized.element_ref.acquisition_cut_ref
                        == receipt.fresh_snapshot.snapshot_cut_ref(),
            },
            comparison_profile_revision: COMPARISON_PROFILE,
            logical_sequence: 503,
        })
        .expect("adapt exact W03 real-provider evidence");
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        assert!(record.observation.failure_flags.is_empty());
        assert_eq!(
            record.observation.eligible_metrics,
            BTreeSet::from([LabMetricKind::Rpomr])
        );

        manager
            .release(session_id)
            .await
            .expect("release W03 runtime observation");
        assert!(manager.status(session_id).await.is_none());
        assert!(bridge.observation_status(session_id).await.is_none());
        seed.shutdown();
    }
}
