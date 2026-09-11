#[cfg(windows)]
mod windows_real_provider_w04 {
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        time::Duration,
    };

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
        WindowsUiaPatternDispatchOperation, WindowsUiaPatternDispatchRequest,
        WindowsUiaPatternSupport, WindowsUiaSnapshotRequest, WindowsUiaWorker,
        WindowsUiaWorkerConfig, WindowsUiaWorkerError,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

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
            let truth = extract_ground_truth(&response);
            assert_eq!(truth.get("terminal").and_then(Value::as_bool), Some(true));
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

    fn truth_str<'a>(truth: &'a Value, field: &str) -> &'a str {
        truth
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("ground truth field {field} must be a string"))
    }

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    fn w04_unsupported_invoke_is_observed_and_rejected_at_dispatch() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let response = seed.command(json!({
            "command": "present_unsupported_invoke_control"
        }));
        assert_eq!(
            response.get("response").and_then(Value::as_str),
            Some("applied")
        );
        let ground_truth = extract_ground_truth(&response);
        assert_eq!(
            ground_truth
                .get("expected_invoke_support")
                .and_then(Value::as_bool),
            Some(false),
            "independent seed oracle must declare that W04 expects no Invoke support"
        );

        let window_handle = truth_u64(&ground_truth, "window_handle");
        let logical_name = truth_str(&ground_truth, "logical_name").to_owned();
        let worker = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 64,
                max_depth: 6,
                max_properties: 512,
            },
            command_timeout: Duration::from_secs(5),
        })
        .expect("spawn dedicated Windows UIA MTA worker");
        let attachment = worker
            .attach(UserSelectedWindowTarget {
                native_window_handle: window_handle,
                expected_process_id: seed.process_id(),
                selection_nonce: Uuid::new_v4(),
            })
            .expect("attach exact external W04 seed window");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:v43:w04:unsupported-invoke".into(),
                    surface_scope: "seed:windows-uia:w04".into(),
                },
            )
            .expect("observe W04 seed through the shipping Windows UIA provider");
        let node = snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(logical_name.as_str()))
            .expect("real W04 control must be present in the semantic snapshot");

        assert_eq!(
            WindowsUiaActionCapabilities::from_node(node).support_for(WindowsUiaPattern::Invoke),
            WindowsUiaPatternSupport::Unsupported,
            "real UIA capability evidence must distinguish explicit Unsupported from Unknown"
        );

        let request = WindowsUiaPatternDispatchRequest {
            dispatch_attempt_ref: Uuid::new_v4(),
            action_id: Uuid::new_v4(),
            preparation_journal_sequence: 1,
            preparation_receipt_ref: "prepare:v43:w04:unsupported-invoke".into(),
            snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            element_ref: node.element_ref.clone(),
            required_pattern: WindowsUiaPattern::Invoke,
            dispatch_operation: WindowsUiaPatternDispatchOperation::Invoke,
            context_requirements: WindowsUiaDispatchContextRequirements {
                require_foreground_target: false,
                require_exact_element_focus: false,
                require_no_modal_blocker: true,
            },
        };
        let error = worker
            .dispatch_pattern(&attachment, request)
            .expect_err("W04 must never produce successful Invoke dispatch evidence");
        assert_eq!(
            error,
            WindowsUiaWorkerError::PatternUnavailable {
                pattern: WindowsUiaPattern::Invoke,
            },
            "the final provider dispatch boundary must reject the unsupported Invoke pattern"
        );

        seed.shutdown();
    }
}
