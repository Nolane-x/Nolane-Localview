#[cfg(windows)]
mod windows_real_provider_w04 {
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        time::Duration,
    };

    use localview_live_bridge::{
        ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, LiveBridge,
    };
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::PrincipalRef;
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, WindowsUiaActionPreflightError,
        WindowsUiaActionPreflightRequest, spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaPattern, WindowsUiaPatternSupport,
        WindowsUiaWorkerConfig,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    const W04_CONTROL_NAME: &str = "LocalView W04 Unsupported Invoke";

    struct SeedProcess {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
        ready_ground_truth: Value,
        shutdown: bool,
    }

    impl SeedProcess {
        fn spawn() -> Self {
            let binary = std::env::var_os("LOCALVIEW_UIA_SEED_BIN")
                .expect("LOCALVIEW_UIA_SEED_BIN must point to the isolated seed executable");
            let mut child = Command::new(binary)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch isolated Windows UIA seed process");
            let stdin = child.stdin.take().expect("seed stdin must be piped");
            let stdout = child.stdout.take().expect("seed stdout must be piped");
            let mut process = Self {
                child,
                stdin,
                stdout: BufReader::new(stdout),
                ready_ground_truth: Value::Null,
                shutdown: false,
            };
            let ready = process.read_response();
            assert_eq!(ready.get("response").and_then(Value::as_str), Some("ready"));
            process.ready_ground_truth = extract_ground_truth(&ready);
            process
        }

        fn process_id(&self) -> u32 {
            self.child.id()
        }

        fn command(&mut self, command: Value) -> Value {
            serde_json::to_writer(&mut self.stdin, &command).expect("serialize seed command");
            writeln!(self.stdin).expect("terminate seed command JSON line");
            self.stdin.flush().expect("flush seed command");
            self.read_response()
        }

        fn read_response(&mut self) -> Value {
            let mut line = String::new();
            self.stdout
                .read_line(&mut line)
                .expect("read seed JSON-line response");
            assert!(
                !line.trim().is_empty(),
                "seed process closed its oracle channel unexpectedly"
            );
            serde_json::from_str(&line).expect("parse seed JSON-line response")
        }

        fn shutdown(mut self) {
            let response = self.command(json!({ "command": "shutdown" }));
            assert_eq!(
                response.get("response").and_then(Value::as_str),
                Some("applied")
            );
            self.shutdown = true;
            let status = self.child.wait().expect("wait for seed process shutdown");
            assert!(status.success(), "seed process must exit cleanly: {status}");
        }
    }

    impl Drop for SeedProcess {
        fn drop(&mut self) {
            if !self.shutdown {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    fn extract_ground_truth(response: &Value) -> Value {
        response
            .get("ground_truth")
            .cloned()
            .expect("seed response must carry independent ground truth")
    }

    fn truth_u64(truth: &Value, field: &str) -> u64 {
        truth
            .get(field)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("ground truth field {field} must be u64"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    async fn w04_unsupported_invoke_stays_typed_and_side_effect_free() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");

        // This is intentionally an independent oracle requirement. The RED
        // lineage fails until the seed exposes a second real Win32 control whose
        // provider does not advertise Invoke and binds that handle into truth.
        let unsupported_handle = truth_u64(
            &seed.ready_ground_truth,
            "unsupported_invoke_control_handle",
        );
        assert_ne!(unsupported_handle, 0, "oracle handle must identify a real control");
        assert_eq!(
            truth_u64(
                &seed.ready_ground_truth,
                "unsupported_invoke_side_effect_count",
            ),
            0,
            "W04 oracle must begin with zero unsupported-Invoke side effects"
        );

        let manager = spawn_windows_uia_runtime_manager(
            LiveBridge::new(128, 16),
            WindowsUiaWorkerConfig {
                snapshot_budget: SnapshotBudget {
                    max_nodes: 96,
                    max_depth: 8,
                    max_properties: 1024,
                },
                command_timeout: Duration::from_secs(5),
            },
            WindowsObserveRuntimeConfig {
                event_capacity: 8,
                drain_limit: 32,
            },
        )
        .expect("spawn production Windows UIA runtime");

        let session_id = Uuid::new_v4();
        let status = manager
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: seed.process_id(),
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach exact W04 seed window through production runtime");
        let snapshot = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W04 initial semantic snapshot must exist");
        let target = snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(W04_CONTROL_NAME))
            .expect("real Windows UIA snapshot must expose the W04 non-invokable control");

        assert_eq!(
            WindowsUiaActionCapabilities::from_node(target).support_for(WindowsUiaPattern::Invoke),
            WindowsUiaPatternSupport::Unsupported,
            "real provider capability evidence must type Invoke as unsupported"
        );

        let authority = ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:w04"),
            acting_principal_ref: PrincipalRef::from("principal:acting:w04"),
            authorization_revision: "authorization:w04:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: status.provider_incarnation_ref.clone(),
            target_incarnation_ref: status.target_incarnation_ref.clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:w04:no-side-effect".into()],
        };
        let result = manager
            .preflight_uia_action(
                session_id,
                WindowsUiaActionPreflightRequest {
                    authority,
                    element_ref: target.element_ref.clone(),
                    required_pattern: WindowsUiaPattern::Invoke,
                },
            )
            .await;
        assert_eq!(
            result,
            Err(WindowsUiaActionPreflightError::PatternUnsupported {
                pattern: WindowsUiaPattern::Invoke,
            }),
            "unsupported real-provider capability must fail closed before dispatch"
        );

        let after = seed.command(json!({ "command": "get_ground_truth" }));
        let after_truth = extract_ground_truth(&after);
        assert_eq!(
            truth_u64(&after_truth, "unsupported_invoke_side_effect_count"),
            0,
            "typed unsupported preflight must not cause a provider or fallback side effect"
        );

        manager
            .release(session_id)
            .await
            .expect("release W04 real-provider observation");
        seed.shutdown();
    }
}
