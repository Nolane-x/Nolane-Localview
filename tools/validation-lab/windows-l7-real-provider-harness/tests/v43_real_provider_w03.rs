#[cfg(windows)]
mod windows_real_provider_w03 {
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::ProviderElementRealization;
    use localview_windows_uia_provider::{
        WindowsUiaItemLookupProperty, WindowsUiaSnapshotRequest,
        WindowsUiaVirtualizedItemQueryRequest, WindowsUiaVirtualizedItemRealizeRequest,
        WindowsUiaWorker, WindowsUiaWorkerConfig,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    const W03_CONTAINER_AUTOMATION_ID: &str = "LocalViewW03VirtualizedItems";
    const W03_VIRTUAL_ITEM_NAME: &str = "LocalView Virtual Item 255";

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

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    fn w03_virtualized_item_requires_realize_then_fresh_observation() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let truth = seed.ground_truth();
        assert_eq!(
            truth.get("virtual_item_name").and_then(Value::as_str),
            Some(W03_VIRTUAL_ITEM_NAME)
        );
        assert_eq!(
            truth
                .get("virtual_item_container_generated")
                .and_then(Value::as_bool),
            Some(false),
            "W03 must begin with the tail item genuinely virtualized"
        );
        assert_eq!(
            truth_u64(&truth, "process_id"),
            u64::from(seed.process_id()),
            "independent oracle process identity must match the launched seed"
        );
        let window_handle = truth_u64(&truth, "window_handle");
        assert_ne!(window_handle, 0, "W03 oracle must expose a live WPF HWND");

        let worker = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 128,
                max_depth: 12,
                max_properties: 2048,
            },
            command_timeout: Duration::from_secs(5),
        })
        .expect("spawn production Windows UIA worker");
        let attachment = worker
            .attach(UserSelectedWindowTarget {
                native_window_handle: window_handle,
                expected_process_id: seed.process_id(),
                selection_nonce: Uuid::new_v4(),
            })
            .expect("attach exact W03 WPF seed window");

        let first_cut = format!("w03:before:{}", Uuid::new_v4());
        let first_snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: first_cut.clone(),
                    surface_scope: "surface:w03:wpf-edge-seed".into(),
                },
            )
            .expect("capture W03 pre-realization snapshot");
        let container = first_snapshot
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(W03_CONTAINER_AUTOMATION_ID))
            .expect("real UIA snapshot must expose the deterministic W03 ItemContainer");
        assert_eq!(
            container.element_ref.realization,
            ProviderElementRealization::RealizedCurrent
        );

        let query = WindowsUiaVirtualizedItemQueryRequest::new(
            first_cut.clone(),
            container.element_ref.clone(),
            WindowsUiaItemLookupProperty::Name,
            W03_VIRTUAL_ITEM_NAME,
        )
        .expect("construct exact W03 ItemContainer query");
        let placeholder = worker
            .query_virtualized_item(&attachment, query)
            .expect("real UIA ItemContainer lookup must return the W03 virtualized placeholder");
        assert_eq!(placeholder.snapshot_cut_ref, first_cut);
        assert_eq!(
            placeholder.provider_incarnation_ref,
            *worker.provider_incarnation_ref()
        );
        assert_eq!(
            placeholder.target_incarnation_ref,
            *attachment.target_incarnation_ref()
        );
        assert_eq!(
            placeholder.placeholder_element_ref.realization,
            ProviderElementRealization::RealizationRequired,
            "a provider-virtualized placeholder must never be minted as RealizedCurrent"
        );
        assert_eq!(
            placeholder.placeholder_element_ref.acquisition_cut_ref,
            placeholder.snapshot_cut_ref
        );
        assert!(
            !seed.virtual_item_generated(),
            "ItemContainer lookup alone must not realize the WPF item"
        );

        let realize_request = WindowsUiaVirtualizedItemRealizeRequest::new(
            placeholder.snapshot_cut_ref.clone(),
            placeholder.placeholder_element_ref.clone(),
        )
        .expect("construct exact W03 realization request");
        let realize_receipt = worker
            .realize_virtualized_item(&attachment, realize_request)
            .expect("real UIA VirtualizedItem::Realize must succeed for W03");
        assert_eq!(
            realize_receipt.previous_placeholder_ref,
            placeholder.placeholder_element_ref,
            "realization receipt must preserve the stale placeholder identity rather than upgrading it"
        );
        assert_eq!(
            realize_receipt.previous_placeholder_ref.realization,
            ProviderElementRealization::RealizationRequired
        );

        seed.wait_until_virtual_item_generated(Duration::from_secs(2));

        let second_cut = format!("w03:after:{}", Uuid::new_v4());
        assert_ne!(second_cut, placeholder.snapshot_cut_ref);
        let fresh_snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: second_cut.clone(),
                    surface_scope: "surface:w03:wpf-edge-seed".into(),
                },
            )
            .expect("capture required fresh post-realization snapshot");
        let realized = fresh_snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(W03_VIRTUAL_ITEM_NAME))
            .expect("fresh real-provider snapshot must contain the realized W03 item");
        assert_eq!(realized.element_ref.realization, ProviderElementRealization::RealizedCurrent);
        assert_eq!(realized.element_ref.acquisition_cut_ref, second_cut);
        assert_ne!(
            realized.element_ref.acquisition_cut_ref,
            realize_receipt.previous_placeholder_ref.acquisition_cut_ref,
            "old placeholder cut must never become fresh action authority"
        );

        drop(worker);
        seed.shutdown();
    }
}
