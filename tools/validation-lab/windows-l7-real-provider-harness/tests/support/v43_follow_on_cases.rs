use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use localview_live_bridge::LiveBridge;
use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
use localview_protocol::ProviderElementRealization;
use localview_validation_lab::{
    RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth, RealProviderLabRecord,
    RealProviderObservedOutcome, adapt_real_provider_case, canonical_digest,
};
use localview_windows_observe_runtime::{
    WindowsObserveRuntimeConfig, spawn_windows_uia_runtime_manager,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaDispatchContextRequirements, WindowsUiaItemLookupProperty,
    WindowsUiaPattern, WindowsUiaPatternDispatchOperation, WindowsUiaPatternDispatchRequest,
    WindowsUiaPatternSupport, WindowsUiaSnapshotRequest, WindowsUiaVirtualizedItemQueryRequest,
    WindowsUiaWorker, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
use serde_json::{Value, json};
use uuid::Uuid;

const W03_CONTAINER_AUTOMATION_ID: &str = "LocalViewW03VirtualizedItems";
const W03_VIRTUAL_ITEM_NAME: &str = "LocalView Virtual Item 255";
const W05_HOSTILE_PROVIDER_NAME: &str = "LocalView W05 Hostile Provider";

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

    fn arm_provider_hang(&mut self) {
        let response = self.command(json!({ "command": "arm_provider_hang" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(response.get("hang_armed").and_then(Value::as_bool), Some(true));
    }

    fn provider_call_entered(&mut self) -> bool {
        let response = self.command(json!({ "command": "get_provider_hang_state" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        response
            .get("provider_call_entered")
            .and_then(Value::as_bool)
            .expect("W05 oracle must report provider_call_entered")
    }

    fn release_provider_hang(&mut self) -> Value {
        let response = self.command(json!({ "command": "release_provider_hang" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(response.get("hang_armed").and_then(Value::as_bool), Some(false));
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

struct ClassicSeedProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    ready_ground_truth: Value,
    shutdown: bool,
}

impl ClassicSeedProcess {
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
        assert_eq!(response.get("response").and_then(Value::as_str), Some("applied"));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for seed process shutdown");
        assert!(status.success(), "seed process must exit cleanly: {status}");
    }
}

impl Drop for ClassicSeedProcess {
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
        .unwrap_or_else(|| panic!("ground-truth field {field} must be u64"))
}

fn truth_str<'a>(truth: &'a Value, field: &str) -> &'a str {
    truth
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("ground-truth field {field} must be a string"))
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

fn selection(process_id: u32, window_handle: u64) -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: window_handle,
        expected_process_id: process_id,
        selection_nonce: Uuid::new_v4(),
    }
}

pub async fn run_w03(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
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
        Some(false)
    );
    assert_eq!(
        truth_u64(&initial_truth, "process_id"),
        u64::from(seed.process_id())
    );
    let window_handle = truth_u64(&initial_truth, "window_handle");

    let bridge = LiveBridge::new(128, 16);
    let manager = spawn_windows_uia_runtime_manager(
        bridge.clone(),
        worker_config(Duration::from_secs(5)),
        WindowsObserveRuntimeConfig {
            event_capacity: 32,
            drain_limit: 32,
        },
    )
    .expect("spawn production Windows UIA runtime for W03 campaign");
    let session_id = Uuid::new_v4();
    manager
        .attach(session_id, selection(seed.process_id(), window_handle))
        .await
        .expect("attach W03 edge seed through production runtime");
    let before = manager
        .current_semantic_snapshot(session_id)
        .await
        .expect("W03 initial runtime snapshot must exist");
    let container = before
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(W03_CONTAINER_AUTOMATION_ID))
        .expect("W03 current snapshot must expose ItemContainer");
    let query = WindowsUiaVirtualizedItemQueryRequest::new(
        before.snapshot_cut_ref(),
        container.element_ref.clone(),
        WindowsUiaItemLookupProperty::Name,
        W03_VIRTUAL_ITEM_NAME,
    )
    .expect("construct W03 exact runtime query");
    let receipt = manager
        .realize_virtualized_item_and_refresh(session_id, query)
        .await
        .expect("W03 runtime realization must produce a fresh reconciled cut");

    seed.wait_until_virtual_item_generated(Duration::from_secs(2));
    let final_truth = seed.ground_truth();
    assert_eq!(
        final_truth
            .get("virtual_item_container_generated")
            .and_then(Value::as_bool),
        Some(true)
    );
    let realized = receipt
        .fresh_snapshot
        .nodes()
        .iter()
        .find(|node| node.name.as_deref() == Some(W03_VIRTUAL_ITEM_NAME))
        .expect("W03 fresh runtime snapshot must contain realized tail item");
    let placeholder_blocked_before_realization = receipt
        .query_receipt
        .placeholder_element_ref
        .realization
        == ProviderElementRealization::RealizationRequired;
    let fresh_cut_after_realization = receipt.fresh_snapshot.snapshot_cut_ref()
        != before.snapshot_cut_ref();
    let realized_current_after_fresh_cut =
        realized.element_ref.realization == ProviderElementRealization::RealizedCurrent
            && realized.element_ref.acquisition_cut_ref == receipt.fresh_snapshot.snapshot_cut_ref();
    assert!(placeholder_blocked_before_realization);
    assert!(fresh_cut_after_realization);
    assert!(realized_current_after_fresh_cut);

    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W03-virtualized-item-realization",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:before:{}", before.observed_digest()),
            format!("windows-uia:after:{}", receipt.fresh_snapshot.observed_digest()),
            format!("windows-uia:reconciliation:{}", receipt.reconciliation_receipt_ref),
            format!("windows-uia:placeholder-cut:{}", receipt.query_receipt.snapshot_cut_ref),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "virtual-item-realized-under-fresh-cut".into(),
            digest: canonical_digest(&final_truth).expect("digest W03 independent oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "virtual-item-realized-under-fresh-cut".into(),
        ),
        case_kind: RealProviderCaseKind::W03VirtualizedItemRealization {
            placeholder_blocked_before_realization,
            fresh_cut_after_realization,
            realized_current_after_fresh_cut,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W03 real-provider campaign evidence");

    manager.release(session_id).await.expect("release W03 runtime");
    assert!(bridge.observation_status(session_id).await.is_none());
    seed.shutdown();
    record
}

pub fn run_w04(
    classic_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed = ClassicSeedProcess::spawn();
    let response = seed.command(json!({ "command": "present_unsupported_invoke_control" }));
    assert_eq!(response.get("response").and_then(Value::as_str), Some("applied"));
    let ground_truth = extract_ground_truth(&response);
    assert_eq!(
        ground_truth
            .get("expected_invoke_support")
            .and_then(Value::as_bool),
        Some(false)
    );
    let window_handle = truth_u64(&ground_truth, "window_handle");
    let logical_name = truth_str(&ground_truth, "logical_name").to_owned();

    let worker = WindowsUiaWorker::spawn(worker_config(Duration::from_secs(5)))
        .expect("spawn W04 production Windows UIA worker");
    let attachment = worker
        .attach(selection(seed.process_id(), window_handle))
        .expect("attach exact W04 seed window");
    let snapshot = worker
        .snapshot(
            &attachment,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: format!("w04:campaign:{}", Uuid::new_v4()),
                surface_scope: "seed:windows-uia:w04".into(),
            },
        )
        .expect("observe W04 through production provider");
    let node = snapshot
        .nodes()
        .iter()
        .find(|node| node.name.as_deref() == Some(logical_name.as_str()))
        .expect("W04 semantic snapshot must contain unsupported control");
    let invoke_support =
        WindowsUiaActionCapabilities::from_node(node).support_for(WindowsUiaPattern::Invoke);
    assert_eq!(invoke_support, WindowsUiaPatternSupport::Unsupported);

    let error = worker
        .dispatch_pattern(
            &attachment,
            WindowsUiaPatternDispatchRequest {
                dispatch_attempt_ref: Uuid::new_v4(),
                action_id: Uuid::new_v4(),
                preparation_journal_sequence: 1,
                preparation_receipt_ref: "prepare:v43:w04:campaign".into(),
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
            },
        )
        .expect_err("W04 must reject unsupported Invoke before successful dispatch");
    assert_eq!(
        error,
        WindowsUiaWorkerError::PatternUnavailable {
            pattern: WindowsUiaPattern::Invoke,
        }
    );
    let after_truth_response = seed.command(json!({ "command": "get_ground_truth" }));
    assert_eq!(
        after_truth_response.get("response").and_then(Value::as_str),
        Some("ground_truth")
    );
    let after_truth = extract_ground_truth(&after_truth_response);
    assert_eq!(
        truth_str(&after_truth, "control_incarnation"),
        truth_str(&ground_truth, "control_incarnation")
    );
    assert_eq!(
        truth_str(&after_truth, "logical_name"),
        truth_str(&ground_truth, "logical_name")
    );

    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W04-unsupported-invoke-pattern",
        seed_app_digest: classic_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:snapshot:{}", snapshot.observed_digest()),
            format!(
                "windows-uia:provider:{}",
                attachment.provider_incarnation_ref().as_str()
            ),
            "windows-uia:invoke-support:unsupported".into(),
            "windows-uia:dispatch-result:pattern-unavailable".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "invoke-unsupported-no-successful-dispatch".into(),
            digest: canonical_digest(&after_truth).expect("digest W04 independent oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "invoke-unsupported-no-successful-dispatch".into(),
        ),
        case_kind: RealProviderCaseKind::W04UnsupportedInvoke {
            invoke_support_unsupported: invoke_support == WindowsUiaPatternSupport::Unsupported,
            dispatch_attempted: false,
            side_effect_observed: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W04 real-provider campaign evidence");

    drop(worker);
    seed.shutdown();
    record
}

pub fn run_w05(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed = EdgeSeedProcess::spawn();
    let initial_truth = seed.ground_truth();
    assert_eq!(
        truth_u64(&initial_truth, "process_id"),
        u64::from(seed.process_id())
    );
    let window_handle = truth_u64(&initial_truth, "window_handle");
    let command_timeout = Duration::from_millis(350);

    let worker_a = WindowsUiaWorker::spawn(worker_config(command_timeout))
        .expect("spawn W05 provider worker A");
    let attachment_a = worker_a
        .attach(selection(seed.process_id(), window_handle))
        .expect("attach W05 edge seed before arming provider hang");
    let provider_a = attachment_a.provider_incarnation_ref().clone();
    let before = worker_a
        .snapshot(
            &attachment_a,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: format!("w05:campaign:baseline:{}", Uuid::new_v4()),
                surface_scope: "surface:w05:wpf-edge-seed".into(),
            },
        )
        .expect("W05 baseline snapshot must succeed");
    let old_ref = before
        .nodes()
        .iter()
        .find(|node| node.name.as_deref() == Some(W05_HOSTILE_PROVIDER_NAME))
        .expect("W05 baseline must contain hostile provider element")
        .element_ref
        .clone();

    seed.arm_provider_hang();
    let timeout_started = Instant::now();
    let timeout_error = worker_a
        .snapshot(
            &attachment_a,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: format!("w05:campaign:hung:{}", Uuid::new_v4()),
                surface_scope: "surface:w05:wpf-edge-seed".into(),
            },
        )
        .expect_err("W05 hostile provider read must hit command timeout");
    let timeout_elapsed = timeout_started.elapsed();
    assert_eq!(timeout_error, WindowsUiaWorkerError::CommandTimeout);
    assert!(timeout_elapsed >= command_timeout);
    assert!(timeout_elapsed < Duration::from_secs(2));
    let provider_call_entered = seed.provider_call_entered();
    assert!(provider_call_entered);

    let poisoned_started = Instant::now();
    let poisoned_error = worker_a
        .snapshot(
            &attachment_a,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: format!("w05:campaign:poisoned:{}", Uuid::new_v4()),
                surface_scope: "surface:w05:wpf-edge-seed".into(),
            },
        )
        .expect_err("poisoned worker must reject reuse");
    let poisoned_elapsed = poisoned_started.elapsed();
    assert_eq!(poisoned_error, WindowsUiaWorkerError::WorkerPoisoned);
    assert!(poisoned_elapsed < command_timeout / 2);

    let release_truth = seed.release_provider_hang();
    drop(worker_a);
    let worker_b = WindowsUiaWorker::spawn(worker_config(command_timeout))
        .expect("spawn W05 fresh provider worker B");
    let attachment_b = worker_b
        .attach(selection(seed.process_id(), window_handle))
        .expect("fresh W05 worker must reacquire same target");
    let provider_b = attachment_b.provider_incarnation_ref().clone();
    assert_ne!(provider_a, provider_b);
    let after = worker_b
        .snapshot(
            &attachment_b,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: format!("w05:campaign:reacquired:{}", Uuid::new_v4()),
                surface_scope: "surface:w05:wpf-edge-seed".into(),
            },
        )
        .expect("fresh W05 worker must recover observation");
    let new_ref = after
        .nodes()
        .iter()
        .find(|node| node.name.as_deref() == Some(W05_HOSTILE_PROVIDER_NAME))
        .expect("W05 reacquired snapshot must contain hostile provider element")
        .element_ref
        .clone();
    assert_eq!(new_ref.provider_incarnation_ref, provider_b);
    let stale_authority_survived_reacquire =
        after.nodes().iter().any(|node| node.element_ref == old_ref);
    assert!(!stale_authority_survived_reacquire);

    let oracle_truth = json!({
        "initial": initial_truth,
        "provider_call_entered": provider_call_entered,
        "release": release_truth,
        "provider_incarnation_changed": provider_a != provider_b,
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W05-windows-uia-provider-hang",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:before:{}", before.observed_digest()),
            format!("windows-uia:after:{}", after.observed_digest()),
            format!("windows-uia:provider-a:{}", provider_a.as_str()),
            format!("windows-uia:provider-b:{}", provider_b.as_str()),
            "windows-uia:first-result:command-timeout".into(),
            "windows-uia:second-result:worker-poisoned".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "provider-hang-quarantined-and-reacquired".into(),
            digest: canonical_digest(&oracle_truth).expect("digest W05 independent oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "provider-hang-quarantined-and-reacquired".into(),
        ),
        case_kind: RealProviderCaseKind::W05ProviderHang {
            caller_returned_bounded: timeout_elapsed < Duration::from_secs(2),
            poisoned_worker_reused: false,
            provider_reacquired: provider_a != provider_b,
            stale_authority_survived_reacquire,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W05 real-provider campaign evidence");

    drop(worker_b);
    seed.shutdown();
    record
}
