#[cfg(windows)]
mod windows_real_provider_seeds {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        thread,
        time::Duration,
    };

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{EventContinuityState, ReconciliationCompleteness};
    use localview_validation_lab::{
        LabMetricKind, RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth,
        RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case, canonical_digest,
        reduce_metric_observations,
    };
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::{
        WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
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
            assert!(!line.trim().is_empty(), "seed process closed its oracle channel unexpectedly");
            serde_json::from_str(&line).expect("parse seed JSON-line response")
        }

        fn shutdown(mut self) {
            let response = self.command(json!({ "command": "shutdown" }));
            assert_eq!(response.get("response").and_then(Value::as_str), Some("applied"));
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    async fn w01_event_gap_requires_reconciliation() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let bridge = LiveBridge::new(128, 16);
        let manager = spawn_windows_uia_runtime_manager(
            bridge,
            WindowsUiaWorkerConfig {
                snapshot_budget: SnapshotBudget {
                    max_nodes: 64,
                    max_depth: 6,
                    max_properties: 512,
                },
                command_timeout: Duration::from_secs(5),
            },
            WindowsObserveRuntimeConfig {
                event_capacity: 1,
                drain_limit: 32,
            },
        )
        .expect("spawn production Windows UIA observe runtime");

        let session_id = Uuid::new_v4();
        let initial_status = manager
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: seed.process_id(),
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach exact external seed window through production runtime");
        assert_eq!(initial_status.event_continuity, EventContinuityState::OrderingOpaque);
        assert_eq!(
            initial_status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        let names = (0..32)
            .map(|index| format!("LocalView W01 Burst {index:02}"))
            .collect::<Vec<_>>();
        let burst = seed.command(json!({
            "command": "burst_name_changes",
            "names": names,
        }));
        assert_eq!(burst.get("response").and_then(Value::as_str), Some("applied"));
        let ground_truth = extract_ground_truth(&burst);
        let final_name = truth_str(&ground_truth, "logical_name").to_owned();
        thread::sleep(Duration::from_millis(300));

        let outcome = manager
            .drain_once(session_id)
            .await
            .expect("drain bounded real UIA callback buffer and reconcile observed gap");
        let accounting = manager
            .resource_accounting(session_id)
            .await
            .expect("runtime accounting must remain live");

        assert!(
            accounting.provider_events_dropped > 0,
            "capacity=1 plus 32 real name changes must retain dropped-event evidence"
        );
        assert_eq!(outcome.report.continuity, EventContinuityState::GapDetected);
        assert_eq!(outcome.status.event_continuity, EventContinuityState::GapDetected);
        assert!(
            outcome.reconciliation_performed,
            "a real provider gap must force a correctness-restoring snapshot"
        );
        assert_eq!(
            outcome.status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );
        assert!(outcome.status.reconciliation_receipt_id.is_some());

        // Observe the reconciled world through the existing production UIA provider,
        // while the seed JSON-line channel remains the independent oracle.
        let observer = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 64,
                max_depth: 6,
                max_properties: 512,
            },
            command_timeout: Duration::from_secs(5),
        })
        .expect("spawn independent production provider observer");
        let attachment = observer
            .attach(UserSelectedWindowTarget {
                native_window_handle: window_handle,
                expected_process_id: seed.process_id(),
                selection_nonce: Uuid::new_v4(),
            })
            .expect("attach provider observer to exact seed window");
        let snapshot = observer
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:v43-real-provider:w01:oracle-compare".into(),
                    surface_scope: "seed:windows-uia:w01".into(),
                },
            )
            .expect("capture provider-backed post-reconciliation snapshot");
        let provider_name = snapshot
            .nodes()
            .iter()
            .filter_map(|node| node.name.as_deref())
            .find(|name| *name == final_name)
            .expect("production UIA snapshot must contain the independent oracle final name")
            .to_owned();

        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest oracle truth");
        let mut evidence_refs = BTreeSet::new();
        evidence_refs.insert(format!(
            "windows-runtime:dropped-events:{}",
            accounting.provider_events_dropped
        ));
        evidence_refs.insert(format!(
            "windows-runtime:reconciliation:{}",
            outcome
                .status
                .reconciliation_receipt_id
                .as_deref()
                .expect("reconciliation receipt must exist")
        ));
        evidence_refs.insert(format!("windows-uia:snapshot:{}", snapshot.observed_digest()));

        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W01-missing-uia-property-event",
            seed_app_digest: "seed-app:windows-uia-seed:task4",
            platform_profile_revision: "windows-uia-hosted-r1",
            environment_artifact_digest: "environment:task4-windows-hosted",
            provider_evidence_refs: evidence_refs,
            ground_truth: RealProviderGroundTruth {
                canonical_outcome: format!("name={final_name}"),
                digest: ground_truth_digest,
            },
            observed_outcome: RealProviderObservedOutcome::Asserted(format!(
                "name={provider_name}"
            )),
            case_kind: RealProviderCaseKind::W01MissingPropertyEvent {
                continuity: outcome.status.event_continuity,
                reconciliation: outcome.status.current_snapshot_completeness,
                accepted_as_fresh: true,
                accepted_as_reconciled: true,
            },
            comparison_profile_revision: "real-provider-exact-r1",
            logical_sequence: truth_u64(&ground_truth, "logical_sequence"),
        })
        .expect("adapt exact W01 real-provider evidence");
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        let metrics = reduce_metric_observations(&[record.observation])
            .expect("reduce W01 real-provider metrics");
        let rpomr = metrics.get(LabMetricKind::Rpomr).expect("RPOMR metric exists");
        assert_eq!((rpomr.numerator, rpomr.denominator), (0, 1));

        manager
            .release(session_id)
            .await
            .expect("release real-provider runtime observation");
        assert!(manager.status(session_id).await.is_none());
        seed.shutdown();
    }
}
