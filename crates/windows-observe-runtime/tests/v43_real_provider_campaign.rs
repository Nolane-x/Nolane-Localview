#[cfg(windows)]
mod windows_l7_real_provider_campaign {
    use std::{
        collections::BTreeSet,
        fs,
        io::{BufRead, BufReader, Write},
        path::{Path, PathBuf},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
        thread,
        time::Duration,
    };

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{EventContinuityState, ReconciliationCompleteness};
    use localview_validation_lab::{
        ActualExecutionAuthority, CampaignLayer, LabMetricKind, LabPreregistration,
        LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
        PersistedPreregistrationReceipt, ProviderCampaignKind, RealProviderCaseInput,
        RealProviderCaseKind, RealProviderGroundTruth, RealProviderLabRecord,
        RealProviderObservedOutcome, ResearchResultClass, ResultEvidence, adapt_real_provider_case,
        canonical_digest, derive_real_provider_campaign_evidence, validate_persisted_receipt,
    };
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, WindowsUiaObserveRuntimeManager,
        spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::WindowsUiaWorkerConfig;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    const PLATFORM_PROFILE: &str = "windows-uia-hosted-r1";
    const COMPARISON_PROFILE: &str = "real-provider-exact-r1";
    const RANDOM_SOURCE_PROFILE: &str = "deterministic-hosted-seed-protocol";
    const CAMPAIGN_START_SEQUENCE: u64 = 100;

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
        event_capacity: usize,
    ) -> WindowsUiaObserveRuntimeManager {
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
                event_capacity,
                drain_limit: 32,
            },
        )
        .expect("spawn production Windows UIA observe runtime")
    }

    fn selection(seed: &SeedProcess, window_handle: u64) -> UserSelectedWindowTarget {
        UserSelectedWindowTarget {
            native_window_handle: window_handle,
            expected_process_id: seed.process_id(),
            selection_nonce: Uuid::new_v4(),
        }
    }

    fn required_env(name: &'static str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be bound by the L7 CI gate"))
    }

    fn environment_value(name: &'static str) -> String {
        std::env::var(name).unwrap_or_else(|_| format!("unknown:{name}-not-exposed"))
    }

    fn seed_executable_digest() -> String {
        let path = required_env("LOCALVIEW_UIA_SEED_BIN");
        let bytes = fs::read(&path).expect("read exact seed executable for digest binding");
        let digest = Sha256::digest(bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    }

    fn artifact_dir() -> PathBuf {
        let path = PathBuf::from(required_env("LOCALVIEW_L7_ARTIFACT_DIR"));
        fs::create_dir_all(&path).expect("create bounded L7 artifact directory");
        path
    }

    fn write_value(path: &Path, value: &Value) {
        let bytes = serde_json::to_vec_pretty(value).expect("serialize L7 JSON artifact");
        fs::write(path, bytes).expect("persist L7 JSON artifact");
    }

    async fn run_w01(
        seed_digest: &str,
        environment_digest: &str,
        logical_sequence: u64,
    ) -> RealProviderLabRecord {
        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let manager = runtime_manager(LiveBridge::new(128, 16), 1);
        let session_id = Uuid::new_v4();

        let initial_status = manager
            .attach(session_id, selection(&seed, window_handle))
            .await
            .expect("attach W01 exact external seed window");
        assert_eq!(
            initial_status.event_continuity,
            EventContinuityState::OrderingOpaque
        );
        assert_eq!(
            initial_status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        let names = (0..32)
            .map(|index| format!("LocalView L7 W01 Burst {index:02}"))
            .collect::<Vec<_>>();
        let mutation_count = names.len() as u64;
        let burst = seed.command(json!({ "command": "burst_name_changes", "names": names }));
        assert_eq!(burst.get("response").and_then(Value::as_str), Some("applied"));
        let ground_truth = extract_ground_truth(&burst);
        let final_name = truth_str(&ground_truth, "logical_name").to_owned();
        thread::sleep(Duration::from_millis(300));

        let outcome = manager
            .drain_once(session_id)
            .await
            .expect("drain bounded W01 provider callbacks");
        let accounting = manager
            .resource_accounting(session_id)
            .await
            .expect("W01 resource accounting remains live");
        assert!(accounting.events_accepted > 0);
        assert!(
            accounting.events_accepted < mutation_count || accounting.provider_events_dropped > 0,
            "W01 must demonstrate incomplete event evidence"
        );
        assert!(outcome.reconciliation_performed);
        assert_eq!(
            outcome.status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        let snapshot = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W01 reconciled snapshot must exist");
        let provider_name = snapshot
            .nodes()
            .iter()
            .filter_map(|node| node.name.as_deref())
            .find(|name| *name == final_name)
            .expect("W01 provider snapshot must match independent oracle final name")
            .to_owned();
        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest W01 oracle truth");
        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W01-missing-uia-property-event",
            seed_app_digest: seed_digest,
            platform_profile_revision: PLATFORM_PROFILE,
            environment_artifact_digest: environment_digest,
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:snapshot:{}", snapshot.observed_digest()),
                format!("windows-runtime:accepted-events:{}", accounting.events_accepted),
                format!("windows-runtime:dropped-events:{}", accounting.provider_events_dropped),
                format!(
                    "seed:logical-sequence:{}",
                    truth_u64(&ground_truth, "logical_sequence")
                ),
            ]),
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
            comparison_profile_revision: COMPARISON_PROFILE,
            logical_sequence,
        })
        .expect("adapt W01 prospective L7 evidence");

        manager
            .release(session_id)
            .await
            .expect("release W01 runtime observation");
        assert!(manager.status(session_id).await.is_none());
        seed.shutdown();
        record
    }

    async fn run_w02(
        seed_digest: &str,
        environment_digest: &str,
        logical_sequence: u64,
    ) -> RealProviderLabRecord {
        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let initial_name = truth_str(&seed.ready_ground_truth, "logical_name").to_owned();
        let initial_control_handle = truth_u64(&seed.ready_ground_truth, "control_handle");
        let initial_control_incarnation =
            truth_str(&seed.ready_ground_truth, "control_incarnation").to_owned();
        let manager = runtime_manager(LiveBridge::new(128, 16), 1);
        let session_id = Uuid::new_v4();

        let initial_status = manager
            .attach(session_id, selection(&seed, window_handle))
            .await
            .expect("attach W02 exact external seed window");
        let before = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W02 initial snapshot must exist");
        let old_ref = before
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(initial_name.as_str()))
            .expect("W02 initial snapshot must contain seed control")
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
            initial_control_handle
        );
        assert_ne!(
            truth_str(&ground_truth, "control_incarnation"),
            initial_control_incarnation
        );
        thread::sleep(Duration::from_millis(300));

        manager
            .release(session_id)
            .await
            .expect("release pre-recreation W02 authority");
        let reattached_status = manager
            .attach(session_id, selection(&seed, window_handle))
            .await
            .expect("reacquire W02 through same provider worker");
        assert_eq!(
            reattached_status.provider_incarnation_ref,
            initial_status.provider_incarnation_ref,
            "W02 must remain inside one live provider incarnation"
        );
        assert_eq!(
            reattached_status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        let after = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("W02 reacquired snapshot must exist");
        let new_ref = after
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some(initial_name.as_str()))
            .expect("W02 reacquired snapshot must contain recreated control")
            .element_ref
            .clone();
        assert_eq!(
            old_ref.provider_incarnation_ref,
            new_ref.provider_incarnation_ref
        );
        assert_ne!(old_ref.acquisition_cut_ref, new_ref.acquisition_cut_ref);
        let accepted_previous_identity_as_current =
            after.nodes().iter().any(|node| node.element_ref == old_ref);
        assert!(!accepted_previous_identity_as_current);
        let provider_identity_reuse_observed =
            old_ref.opaque_provider_element_id == new_ref.opaque_provider_element_id;
        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest W02 oracle truth");
        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W02-recreated-uia-element",
            seed_app_digest: seed_digest,
            platform_profile_revision: PLATFORM_PROFILE,
            environment_artifact_digest: environment_digest,
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!("windows-uia:after:{}", after.observed_digest()),
                format!(
                    "seed:logical-sequence:{}",
                    truth_u64(&ground_truth, "logical_sequence")
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
            comparison_profile_revision: COMPARISON_PROFILE,
            logical_sequence,
        })
        .expect("adapt W02 prospective L7 evidence");

        manager
            .release(session_id)
            .await
            .expect("release W02 runtime observation");
        assert!(manager.status(session_id).await.is_none());
        seed.shutdown();
        record
    }

    async fn run_w06(
        seed_digest: &str,
        environment_digest: &str,
        logical_sequence: u64,
    ) -> RealProviderLabRecord {
        let mut seed = SeedProcess::spawn();
        let window_handle = truth_u64(&seed.ready_ground_truth, "window_handle");
        let logical_name = truth_str(&seed.ready_ground_truth, "logical_name").to_owned();
        let bridge = LiveBridge::new(128, 16);
        let session_id = Uuid::new_v4();

        let manager_a = runtime_manager(bridge.clone(), 8);
        let status_a = manager_a
            .attach(session_id, selection(&seed, window_handle))
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
            .expect("W06 incarnation A snapshot must contain seed control")
            .element_ref
            .clone();

        manager_a
            .release(session_id)
            .await
            .expect("release W06 provider incarnation A");
        assert!(manager_a.status(session_id).await.is_none());
        assert!(bridge.observation_status(session_id).await.is_none());
        drop(manager_a);

        let manager_b = runtime_manager(bridge.clone(), 8);
        let status_b = manager_b
            .attach(session_id, selection(&seed, window_handle))
            .await
            .expect("reacquire W06 provider incarnation B");
        assert_ne!(
            status_a.provider_incarnation_ref,
            status_b.provider_incarnation_ref,
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
            .expect("W06 incarnation B snapshot must contain same seed control")
            .element_ref
            .clone();
        assert_eq!(
            new_ref.provider_incarnation_ref,
            status_b.provider_incarnation_ref
        );
        let stale_authority_survived_reacquire =
            after.nodes().iter().any(|node| node.element_ref == old_ref);
        assert!(!stale_authority_survived_reacquire);

        let oracle = seed.command(json!({ "command": "get_ground_truth" }));
        assert_eq!(
            oracle.get("response").and_then(Value::as_str),
            Some("ground_truth")
        );
        let ground_truth = extract_ground_truth(&oracle);
        let ground_truth_digest = canonical_digest(&ground_truth).expect("digest W06 oracle truth");
        manager_b
            .release(session_id)
            .await
            .expect("release W06 provider incarnation B");
        let cleanup_to_baseline = manager_b.status(session_id).await.is_none()
            && bridge.observation_status(session_id).await.is_none();
        assert!(cleanup_to_baseline);

        let record = adapt_real_provider_case(RealProviderCaseInput {
            case_id: "W06-windows-uia-provider-reacquire",
            seed_app_digest: seed_digest,
            platform_profile_revision: PLATFORM_PROFILE,
            environment_artifact_digest: environment_digest,
            provider_evidence_refs: BTreeSet::from([
                format!("windows-uia:before:{}", before.observed_digest()),
                format!("windows-uia:after:{}", after.observed_digest()),
                format!(
                    "windows-uia:provider-a:{}",
                    status_a.provider_incarnation_ref.as_str()
                ),
                format!(
                    "windows-uia:provider-b:{}",
                    status_b.provider_incarnation_ref.as_str()
                ),
                format!(
                    "seed:logical-sequence:{}",
                    truth_u64(&ground_truth, "logical_sequence")
                ),
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
            comparison_profile_revision: COMPARISON_PROFILE,
            logical_sequence,
        })
        .expect("adapt W06 prospective L7 evidence");
        seed.shutdown();
        record
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires hosted Windows UIA provider, seed executable, and CI environment authority"]
    async fn prospective_l7_campaign_binds_w01_w02_w06_to_exact_candidate() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider campaign execution must be explicitly enabled"
        );

        let candidate_sha = required_env("LOCALVIEW_CANDIDATE_SHA");
        let seed_digest = seed_executable_digest();
        let artifact_dir = artifact_dir();
        let architecture = std::env::var("RUNNER_ARCH")
            .unwrap_or_else(|_| std::env::consts::ARCH.to_owned());
        let environment = json!({
            "windows_build": environment_value("LOCALVIEW_WINDOWS_BUILD"),
            "architecture": architecture,
            "localview_candidate_sha": candidate_sha,
            "provider_profile_revision": PLATFORM_PROFILE,
            "display_topology": environment_value("LOCALVIEW_DISPLAY_TOPOLOGY"),
            "dpi_scale": environment_value("LOCALVIEW_DPI_SCALE"),
            "locale_input_method": environment_value("LOCALVIEW_LOCALE_INPUT_METHOD"),
            "permission_state": environment_value("LOCALVIEW_PERMISSION_STATE"),
            "seed_executable_digest": seed_digest,
        });
        write_value(&artifact_dir.join("environment-manifest.json"), &environment);
        let environment_digest =
            canonical_digest(&environment).expect("digest canonical environment manifest");

        let seed_catalog_digest = canonical_digest(&json!({
            "candidate_sha": required_env("LOCALVIEW_CANDIDATE_SHA"),
            "environment_digest": environment_digest.0.clone(),
            "seed_executable_digest": seed_digest.clone(),
            "required_cases": ["W01", "W02", "W06"],
        }))
        .expect("digest L7 seed catalog");
        let preregistration = LabPreregistration {
            revision_context: LabRevisionContext {
                lab_revision: "lab-v43-windows-l7-r1".into(),
                seed_corpus_revision: "windows-provider-seeds-w01-w02-w06-r1".into(),
                spec_revision_digest: "v4.3-principal-provider-reconciliation-closure".into(),
                reference_reducer_revision: "provider-oracle-r1".into(),
                mutation_catalog_revision: "windows-provider-seed-matrix-r1".into(),
                comparison_profile_revision: COMPARISON_PROFILE.into(),
                random_source_profile: RANDOM_SOURCE_PROFILE.into(),
                platform_profile: Some(PLATFORM_PROFILE.into()),
                start_sequence: CAMPAIGN_START_SEQUENCE,
            },
            seed_catalog_digest: seed_catalog_digest.clone(),
            seed_identities: vec![
                LabSeedIdentity {
                    seed_id: "W01".into(),
                    prediction_revision: "w01-missing-property-event-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: "W02".into(),
                    prediction_revision: "w02-recreated-element-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: "W06".into(),
                    prediction_revision: "w06-provider-reacquire-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
            ],
            campaign_layer: CampaignLayer::L7,
            expected_distinctions: BTreeSet::from([
                "event continuity != current snapshot completeness".into(),
                "element incarnation != provider incarnation".into(),
                "provider reacquire invalidates old authority".into(),
            ]),
            model_bound: None,
            assumptions: BTreeSet::from([
                "hosted Windows runner exposes real Win32 UI Automation".into(),
                "production LocalView never reads the seed oracle pipe".into(),
            ]),
            declared_metrics: BTreeSet::from([
                LabMetricKind::Rpomr,
                LabMetricKind::Eoffr,
                LabMetricKind::Rmr,
                LabMetricKind::Piaer,
                LabMetricKind::Scar,
                LabMetricKind::Cbfr,
            ]),
            creation_sequence: 80,
        };

        let prepared = preregistration
            .prepare()
            .expect("prepare L7 preregistration");
        let prereg_path = artifact_dir.join("LAB-PREREGISTRATION.json");
        fs::write(&prereg_path, &prepared.canonical_bytes)
            .expect("persist preregistration before campaign start");
        let persisted_receipt = PersistedPreregistrationReceipt {
            digest: prepared.digest.clone(),
            logical_sequence: 90,
            persistence_ref: prereg_path.to_string_lossy().into_owned(),
        };
        let receipt_bytes = serde_json::to_vec_pretty(&persisted_receipt)
            .expect("serialize preregistration receipt");
        fs::write(
            artifact_dir.join("LAB-PREREGISTRATION-RECEIPT.json"),
            receipt_bytes,
        )
        .expect("persist preregistration receipt");
        let receipt = validate_persisted_receipt(&prepared, persisted_receipt)
            .expect("validate persisted preregistration receipt");
        let authority = ActualExecutionAuthority {
            seed_catalog_digest,
            comparison_profile_revision: COMPARISON_PROFILE.into(),
            random_source_profile: RANDOM_SOURCE_PROFILE.into(),
            model_bound: None,
        };
        let admission = LabRunAdmission::Prospective {
            preregistration: preregistration.clone(),
            receipt,
        };
        let mut run = LabRunBuilder::start_provider_campaign(
            ProviderCampaignKind::RealProviderSeedApplications,
            admission,
            authority,
        )
        .expect("start typed prospective L7 real-provider campaign");

        let environment_digest_text = environment_digest.0.clone();
        let records = vec![
            run_w01(&seed_digest, &environment_digest_text, 101).await,
            run_w02(&seed_digest, &environment_digest_text, 102).await,
            run_w06(&seed_digest, &environment_digest_text, 103).await,
        ];
        for record in &records {
            assert_eq!(
                record.result_evidence,
                Some(ResultEvidence::RealProviderIntegrationPass),
                "each required real-provider case must be complete before campaign finalization"
            );
            assert!(record.observation.failure_flags.is_empty());
            run.append_observation(record.observation.clone())
                .expect("append exact provider-backed observation to prospective campaign");
        }

        let campaign_evidence = derive_real_provider_campaign_evidence(&records)
            .expect("all required real-provider records must produce campaign evidence");
        assert_eq!(
            campaign_evidence,
            ResultEvidence::RealProviderIntegrationPass
        );
        let completed = run
            .finalize(campaign_evidence, 104)
            .expect("clean measured L7 campaign may mint scoped real-provider pass");
        let rpomr = completed
            .payload
            .metric_snapshot
            .get(LabMetricKind::Rpomr)
            .expect("RPOMR metric must exist");
        assert_eq!((rpomr.numerator, rpomr.denominator), (0, 3));
        assert_eq!(
            completed.payload.result_class,
            ResearchResultClass::RealProviderIntegrationPass
        );
        assert_eq!(
            completed
                .payload
                .actual_execution_authority
                .seed_catalog_digest,
            preregistration.seed_catalog_digest
        );
        assert_eq!(
            completed.payload.revision_context.platform_profile.as_deref(),
            Some(PLATFORM_PROFILE)
        );
        let completed_bytes =
            serde_json::to_vec_pretty(&completed).expect("serialize completed L7 result");
        fs::write(artifact_dir.join("LAB-RESULT.json"), completed_bytes)
            .expect("persist completed L7 result artifact");
    }
}
