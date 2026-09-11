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
    use localview_windows_uia_provider::WindowsUiaWorkerConfig;
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

    fn runtime_manager(bridge: LiveBridge) -> localview_windows_observe_runtime::WindowsObserveRuntimeManager {
        spawn_windows_uia_runtime_manager(
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
        .expect("spawn production Windows UIA observe runtime")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    async fn w01_missing_or_coalesced_property_events_require_reconciliation() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let manager = runtime_manager(LiveBridge::new(128, 16));

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
        assert_eq!(
            initial_status.event_continuity,
            EventContinuityState::OrderingOpaque
        );
        assert_eq!(
            initial_status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        let names = (0..32)
            .map(|index| format!("LocalView W01 Burst {index:02}"))
            .collect::<Vec<_>>();
        let mutation_count = names.len() as u64;
        let burst = seed.command(json!({
            "command": "burst_name_changes",
            "names": names,
        }));
        assert_eq!(
            burst.get("response").and_then(Value::as_str),
            Some("applied")
        );
        let ground_truth = extract_ground_truth(&burst);
        let final_name = truth_str(&ground_truth, "logical_name").to_owned();
        thread::sleep(Duration::from_millis(300));

        let outcome = manager
            .drain_once(session_id)
            .await
            .expect("drain bounded real UIA callback buffer and reconcile opaque event evidence");
        let accounting = manager
            .resource_accounting(session_id)
            .await
            .expect("runtime accounting must remain live");

        assert!(
            accounting.events_accepted > 0,
            "W01 seed must produce at least one real UIA callback so runtime invalidation is exercised"
        );
        assert!(
            accounting.events_accepted < mutation_count || accounting.provider_events_dropped > 0,
            "W01 must demonstrate incomplete event evidence through provider coalescing or bounded-buffer loss"
        );
        assert!(matches!(
            outcome.report.continuity,
            EventContinuityState::OrderingOpaque | EventContinuityState::GapDetected
        ));
        assert!(
            outcome.reconciliation_performed,
            "incomplete real-provider event evidence must force one correctness-restoring snapshot"
        );
        assert_eq!(
            outcome.status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );
        assert!(outcome.status.reconciliation_receipt_id.is_some());

        let snapshot = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("runtime must retain the reconciled semantic snapshot");
        let provider_name = snapshot
            .nodes()
            .iter()
            .filter_map(|node| node.name.as_deref())
            .find(|name| *name == final_name)
            .expect("runtime reconciled snapshot must contain the independent oracle final name")
            .to_owned();

        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest oracle truth");
        let mut evidence_refs = BTreeSet::new();
        evidence_refs.insert(format!(
            "windows-runtime:accepted-events:{}",
            accounting.events_accepted
        ));
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
        let rpomr = metrics
            .get(LabMetricKind::Rpomr)
            .expect("RPOMR metric exists");
        assert_eq!((rpomr.numerator, rpomr.denominator), (0, 1));

        manager
            .release(session_id)
            .await
            .expect("release real-provider runtime observation");
        assert!(manager.status(session_id).await.is_none());
        seed.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    async fn w02_recreated_element_never_resurrects_stale_identity() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let initial_name = truth_str(&seed.ready_ground_truth, "logical_name").to_owned();
        let initial_control_handle = truth_u64(&seed.ready_ground_truth, "control_handle");
        let initial_control_incarnation =
            truth_str(&seed.ready_ground_truth, "control_incarnation").to_owned();
        let manager = runtime_manager(LiveBridge::new(128, 16));
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
            .expect("attach exact external W02 seed window");
        let before = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("initial W02 snapshot must exist");
        let old_ref = before
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(initial_name.as_str()))
            .expect("initial snapshot must contain the seed control")
            .element_ref
            .clone();

        let recreated = seed.command(json!({ "command": "recreate_control" }));
        assert_eq!(
            recreated.get("response").and_then(Value::as_str),
            Some("applied")
        );
        let ground_truth = extract_ground_truth(&recreated);
        assert_ne!(
            truth_u64(&ground_truth, "control_handle"),
            initial_control_handle,
            "seed oracle must prove a new native control was created"
        );
        assert_ne!(
            truth_str(&ground_truth, "control_incarnation"),
            initial_control_incarnation,
            "seed oracle must prove the control lifetime changed"
        );
        assert_eq!(truth_u64(&ground_truth, "recreation_generation"), 2);
        thread::sleep(Duration::from_millis(300));

        let outcome = manager
            .drain_once(session_id)
            .await
            .expect("drain W02 structure callbacks and reconcile recreated element");
        assert!(
            outcome.report.ingest.accepted > 0,
            "real control recreation must produce provider invalidation evidence"
        );
        assert!(
            outcome.reconciliation_performed,
            "accepted opaque recreation evidence must invalidate the pre-recreation snapshot"
        );

        let after = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("post-recreation W02 snapshot must exist");
        let new_ref = after
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(initial_name.as_str()))
            .expect("reconciled snapshot must contain the recreated seed control")
            .element_ref
            .clone();

        assert_eq!(
            old_ref.provider_incarnation_ref,
            new_ref.provider_incarnation_ref,
            "W02 isolates element recreation inside one live provider incarnation"
        );
        assert_ne!(
            old_ref.acquisition_cut_ref, new_ref.acquisition_cut_ref,
            "recreated element authority must be reacquired at a new observation cut"
        );
        let accepted_previous_identity_as_current = after
            .nodes()
            .iter()
            .any(|node| node.element_ref == old_ref);
        assert!(
            !accepted_previous_identity_as_current,
            "the exact pre-recreation ProviderElementRef must never authorize the new control"
        );

        let provider_identity_reuse_observed = old_ref.opaque_provider_element_id
            == new_ref.opaque_provider_element_id;
        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest W02 oracle truth");
        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W02-recreated-uia-element",
            seed_app_digest: "seed-app:windows-uia-seed:task5-w02",
            platform_profile_revision: "windows-uia-hosted-r1",
            environment_artifact_digest: "environment:task5-windows-hosted",
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!("windows-uia:after:{}", after.observed_digest()),
                format!(
                    "seed:control-incarnation:{}",
                    truth_str(&ground_truth, "control_incarnation")
                ),
            ]),
            ground_truth: RealProviderGroundTruth {
                canonical_outcome: "stale-element-ref-rejected".into(),
                digest: ground_truth_digest,
            },
            observed_outcome: RealProviderObservedOutcome::Asserted(
                "stale-element-ref-rejected".into(),
            ),
            case_kind: RealProviderCaseKind::W02RecreatedElement {
                previous_provider_incarnation: old_ref.provider_incarnation_ref.clone(),
                current_provider_incarnation: new_ref.provider_incarnation_ref.clone(),
                opaque_provider_element_id: old_ref.opaque_provider_element_id.clone(),
                provider_identity_reuse_observed,
                accepted_previous_identity_as_current,
            },
            comparison_profile_revision: "real-provider-exact-r1",
            logical_sequence: truth_u64(&ground_truth, "logical_sequence"),
        })
        .expect("adapt exact W02 real-provider evidence");
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        if provider_identity_reuse_observed {
            assert!(
                record.observation.eligible_metrics.contains(&LabMetricKind::Piaer),
                "PIAER is eligible only when the real provider reused its opaque element identity"
            );
        } else {
            assert!(
                !record.observation.eligible_metrics.contains(&LabMetricKind::Piaer),
                "absence of provider identity reuse must not be counted as a clean PIAER trial"
            );
        }

        manager
            .release(session_id)
            .await
            .expect("release W02 runtime observation");
        assert!(manager.status(session_id).await.is_none());
        seed.shutdown();
    }
}
