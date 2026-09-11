#[cfg(windows)]
mod windows_real_provider_w06 {
    use std::{
        collections::BTreeSet,
        io::{BufRead, BufReader, Write},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        time::Duration,
    };

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_validation_lab::{
        LabMetricKind, RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth,
        RealProviderObservedOutcome, ResultEvidence, adapt_real_provider_case, canonical_digest,
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

    fn runtime_manager(
        bridge: LiveBridge,
    ) -> localview_windows_observe_runtime::WindowsUiaObserveRuntimeManager {
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
                event_capacity: 8,
                drain_limit: 8,
            },
        )
        .expect("spawn production Windows UIA observe runtime")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and seed executable"]
    async fn w06_provider_reacquire_rebinds_incarnation_and_cleans_up() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let logical_name = truth_str(&seed.ready_ground_truth, "logical_name").to_owned();
        let bridge = LiveBridge::new(128, 16);
        let session_id = Uuid::new_v4();

        let manager_a = runtime_manager(bridge.clone());
        let status_a = manager_a
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: seed.process_id(),
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach W06 provider incarnation A");
        let before = manager_a
            .current_semantic_snapshot(session_id)
            .await
            .expect("W06 incarnation A snapshot must exist");
        let old_ref = before
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(logical_name.as_str()))
            .expect("W06 incarnation A snapshot must contain the seed control")
            .element_ref
            .clone();

        manager_a
            .release(session_id)
            .await
            .expect("release W06 provider incarnation A");
        assert!(manager_a.status(session_id).await.is_none());
        assert!(
            bridge.observation_status(session_id).await.is_none(),
            "release A must remove bridge observation authority before provider reacquire"
        );
        drop(manager_a);

        let manager_b = runtime_manager(bridge.clone());
        let status_b = manager_b
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: seed.process_id(),
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("reacquire W06 provider incarnation B");
        assert_ne!(
            status_a.provider_incarnation_ref, status_b.provider_incarnation_ref,
            "W06 must prove a genuinely new provider worker incarnation"
        );

        let after = manager_b
            .current_semantic_snapshot(session_id)
            .await
            .expect("W06 incarnation B snapshot must exist");
        let new_ref = after
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(logical_name.as_str()))
            .expect("W06 incarnation B snapshot must contain the same live seed control")
            .element_ref
            .clone();
        assert_eq!(
            new_ref.provider_incarnation_ref, status_b.provider_incarnation_ref,
            "reacquired element authority must bind to provider incarnation B"
        );
        let stale_authority_survived_reacquire = after
            .nodes()
            .iter()
            .any(|node| node.element_ref == old_ref);
        assert!(
            !stale_authority_survived_reacquire,
            "no element authority from provider incarnation A may survive reacquire"
        );

        let oracle = seed.command(json!({ "command": "get_ground_truth" }));
        assert_eq!(oracle.get("response").and_then(Value::as_str), Some("ground_truth"));
        let ground_truth = extract_ground_truth(&oracle);
        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest W06 oracle truth");

        manager_b
            .release(session_id)
            .await
            .expect("release W06 provider incarnation B");
        let cleanup_to_baseline = manager_b.status(session_id).await.is_none()
            && bridge.observation_status(session_id).await.is_none();
        assert!(
            cleanup_to_baseline,
            "W06 release B must restore runtime and bridge observation baseline"
        );

        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W06-windows-uia-provider-reacquire",
            seed_app_digest: "seed-app:windows-uia-seed:task5-w06",
            platform_profile_revision: "windows-uia-hosted-r1",
            environment_artifact_digest: "environment:task5-windows-hosted",
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!("windows-uia:after:{}", after.observed_digest()),
                format!("windows-uia:provider-a:{}", status_a.provider_incarnation_ref.as_str()),
                format!("windows-uia:provider-b:{}", status_b.provider_incarnation_ref.as_str()),
            ]),
            ground_truth: RealProviderGroundTruth {
                canonical_outcome: "provider-reacquired-clean".into(),
                digest: ground_truth_digest,
            },
            observed_outcome: RealProviderObservedOutcome::Asserted(
                "provider-reacquired-clean".into(),
            ),
            case_kind: RealProviderCaseKind::W06ProviderReacquire {
                previous_provider_incarnation: status_a.provider_incarnation_ref.clone(),
                current_provider_incarnation: status_b.provider_incarnation_ref.clone(),
                stale_authority_survived_reacquire,
                cleanup_to_baseline,
            },
            comparison_profile_revision: "real-provider-exact-r1",
            logical_sequence: truth_u64(&ground_truth, "logical_sequence"),
        })
        .expect("adapt exact W06 real-provider evidence");
        assert_eq!(
            record.result_evidence,
            Some(ResultEvidence::RealProviderIntegrationPass)
        );
        assert!(record.observation.eligible_metrics.contains(&LabMetricKind::Scar));
        assert!(record.observation.eligible_metrics.contains(&LabMetricKind::Cbfr));

        seed.shutdown();
    }
}
