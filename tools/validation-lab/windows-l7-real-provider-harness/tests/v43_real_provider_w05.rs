#[cfg(windows)]
mod windows_real_provider_w05 {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        time::{Duration, Instant},
    };

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_validation_lab::{
        LabMetricKind, RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth,
        RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case, canonical_digest,
    };
    use localview_windows_uia_provider::{
        WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
    };
    use serde_json::{Value, json};
    use uuid::Uuid;

    const HOSTILE_PROVIDER_NAME: &str = "LocalView W05 Hostile Provider";

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

        fn arm_provider_hang(&mut self) {
            let response = self.command(json!({ "command": "arm_provider_hang" }));
            assert_eq!(
                response.get("ok").and_then(Value::as_bool),
                Some(true),
                "W05 edge seed must expose a deterministic nonblocking arm_provider_hang command: {response}"
            );
            assert_eq!(
                response.get("hang_armed").and_then(Value::as_bool),
                Some(true),
                "W05 oracle must independently confirm that the hostile provider path is armed"
            );
        }

        fn provider_call_entered(&mut self) -> bool {
            let response = self.command(json!({ "command": "get_provider_hang_state" }));
            assert_eq!(
                response.get("ok").and_then(Value::as_bool),
                Some(true),
                "W05 oracle channel must remain responsive while the provider call is blocked: {response}"
            );
            response
                .get("provider_call_entered")
                .and_then(Value::as_bool)
                .expect("W05 oracle must report provider_call_entered")
        }

        fn release_provider_hang(&mut self) -> Value {
            let response = self.command(json!({ "command": "release_provider_hang" }));
            assert_eq!(
                response.get("ok").and_then(Value::as_bool),
                Some(true),
                "W05 oracle must release the hostile provider without shutting down the seed: {response}"
            );
            assert_eq!(
                response.get("hang_armed").and_then(Value::as_bool),
                Some(false),
                "W05 oracle must prove the provider hang is disarmed before reacquire"
            );
            response
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

    fn worker_config(command_timeout: Duration) -> WindowsUiaWorkerConfig {
        WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 128,
                max_depth: 12,
                max_properties: 2048,
            },
            command_timeout,
        }
    }

    fn selection(seed: &EdgeSeedProcess, window_handle: u64) -> UserSelectedWindowTarget {
        UserSelectedWindowTarget {
            native_window_handle: window_handle,
            expected_process_id: seed.process_id(),
            selection_nonce: Uuid::new_v4(),
        }
    }

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider and hostile WPF edge seed"]
    fn w05_provider_hang_poison_requires_fresh_provider_reacquire() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let truth = seed.ground_truth();
        assert_eq!(
            truth_u64(&truth, "process_id"),
            u64::from(seed.process_id()),
            "independent oracle process identity must match the launched seed"
        );
        let window_handle = truth_u64(&truth, "window_handle");
        assert_ne!(window_handle, 0, "W05 oracle must expose a live WPF HWND");

        let command_timeout = Duration::from_millis(350);
        let worker_a = WindowsUiaWorker::spawn(worker_config(command_timeout))
            .expect("spawn production Windows UIA worker A");
        let attachment_a = worker_a
            .attach(selection(&seed, window_handle))
            .expect("attach exact W05 WPF seed window before arming the hostile provider path");
        let provider_a = attachment_a.provider_incarnation_ref().clone();

        let before = worker_a
            .snapshot(
                &attachment_a,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: format!("w05:baseline:{}", Uuid::new_v4()),
                    surface_scope: "surface:w05:wpf-edge-seed".into(),
                },
            )
            .expect("W05 baseline snapshot must succeed before the provider hang is armed");
        let old_ref = before
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(HOSTILE_PROVIDER_NAME))
            .expect("W05 baseline snapshot must contain the hostile provider element")
            .element_ref
            .clone();
        assert_eq!(old_ref.provider_incarnation_ref, provider_a);

        seed.arm_provider_hang();

        let timeout_started = Instant::now();
        let timeout_error = worker_a
            .snapshot(
                &attachment_a,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: format!("w05:hung:{}", Uuid::new_v4()),
                    surface_scope: "surface:w05:wpf-edge-seed".into(),
                },
            )
            .expect_err("armed hostile provider read must cross the production command timeout");
        let timeout_elapsed = timeout_started.elapsed();
        assert_eq!(timeout_error, WindowsUiaWorkerError::CommandTimeout);
        assert!(
            timeout_elapsed >= command_timeout,
            "W05 timeout returned before the configured command budget elapsed: {timeout_elapsed:?}"
        );
        assert!(
            timeout_elapsed < Duration::from_secs(2),
            "W05 caller must remain bounded despite the blocked provider call: {timeout_elapsed:?}"
        );
        assert!(
            seed.provider_call_entered(),
            "independent W05 oracle must prove that a real hostile provider call actually entered the block"
        );

        let poisoned_started = Instant::now();
        let poisoned_error = worker_a
            .snapshot(
                &attachment_a,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: format!("w05:poisoned:{}", Uuid::new_v4()),
                    surface_scope: "surface:w05:wpf-edge-seed".into(),
                },
            )
            .expect_err("timed-out worker must never be reused for another provider command");
        let poisoned_elapsed = poisoned_started.elapsed();
        assert_eq!(poisoned_error, WindowsUiaWorkerError::WorkerPoisoned);
        assert!(
            poisoned_elapsed < command_timeout / 2,
            "poisoned worker must fail fast instead of waiting another provider timeout: {poisoned_elapsed:?}"
        );

        let released_truth = seed.release_provider_hang();
        drop(worker_a);

        let worker_b = WindowsUiaWorker::spawn(worker_config(command_timeout))
            .expect("spawn fresh Windows UIA worker B after poison quarantine");
        let attachment_b = worker_b
            .attach(selection(&seed, window_handle))
            .expect("fresh worker B must reacquire the same still-live W05 seed");
        let provider_b = attachment_b.provider_incarnation_ref().clone();
        assert_ne!(
            provider_a, provider_b,
            "W05 reacquire must mint a genuinely fresh provider incarnation"
        );

        let after = worker_b
            .snapshot(
                &attachment_b,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: format!("w05:reacquired:{}", Uuid::new_v4()),
                    surface_scope: "surface:w05:wpf-edge-seed".into(),
                },
            )
            .expect("fresh worker B must recover provider observation after hang release");
        let new_ref = after
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(HOSTILE_PROVIDER_NAME))
            .expect("W05 reacquired snapshot must contain the hostile provider element")
            .element_ref
            .clone();
        assert_eq!(new_ref.provider_incarnation_ref, provider_b);
        let stale_authority_survived_reacquire =
            after.nodes().iter().any(|node| node.element_ref == old_ref);
        assert!(
            !stale_authority_survived_reacquire,
            "no authority issued by poisoned provider incarnation A may survive worker B reacquire"
        );

        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W05-windows-uia-provider-hang",
            seed_app_digest: "seed-app:windows-uia-edge-seed:w05",
            platform_profile_revision: "windows-uia-hosted-r1",
            environment_artifact_digest: "environment:w05-windows-hosted",
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!("windows-uia:after:{}", after.observed_digest()),
                format!("windows-uia:provider-a:{}", provider_a.as_str()),
                format!("windows-uia:provider-b:{}", provider_b.as_str()),
            ]),
            ground_truth: RealProviderGroundTruth {
                canonical_outcome: "provider-hang-quarantined-and-reacquired".into(),
                digest: canonical_digest(&released_truth).expect("digest W05 independent oracle truth"),
            },
            observed_outcome: RealProviderObservedOutcome::Asserted(
                "provider-hang-quarantined-and-reacquired".into(),
            ),
            case_kind: RealProviderCaseKind::W05ProviderHang {
                caller_returned_bounded: timeout_elapsed < Duration::from_secs(2),
                poisoned_worker_reused: false,
                provider_reacquired: true,
                stale_authority_survived_reacquire,
            },
            comparison_profile_revision: "real-provider-exact-r1",
            logical_sequence: 505,
        })
        .expect("adapt exact W05 real-provider evidence");
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        assert_eq!(
            record.observation.eligible_metrics,
            BTreeSet::from([LabMetricKind::Rpomr])
        );

        drop(worker_b);
        seed.shutdown();
    }
}
