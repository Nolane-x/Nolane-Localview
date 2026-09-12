#![cfg(windows)]

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, DispatchExecutionPermit, DispatchPreparationReceipt,
};
use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
use localview_protocol::{PrincipalRef, SessionId};
use localview_windows_uia_provider::{
    WindowsKeyTransition, WindowsUiaAttachment, WindowsUiaDispatchContextRequirements,
    WindowsUiaSnapshotRequest, WindowsUiaVerifiedInputRequest, WindowsUiaWorker,
    WindowsUiaWorkerConfig, WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub const INPUT_TARGET_AUTOMATION_ID: &str = "LocalViewW07W09VerifiedInputTarget";
pub const SPACE_VK: u16 = 0x20;

pub struct EdgeSeedProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    shutdown: bool,
    shift_owned: bool,
}

impl EdgeSeedProcess {
    pub fn spawn() -> Self {
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
            shift_owned: false,
        }
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    pub fn command(&mut self, command: Value) -> Value {
        self.command_inner(command)
            .expect("edge seed oracle command must return one JSON line")
    }

    fn command_inner(&mut self, command: Value) -> Option<Value> {
        serde_json::to_writer(&mut self.stdin, &command).ok()?;
        writeln!(self.stdin).ok()?;
        self.stdin.flush().ok()?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).ok()?;
        if line.trim().is_empty() {
            return None;
        }
        serde_json::from_str(&line).ok()
    }

    pub fn prepare_input_target(&mut self) -> Value {
        let response = self.command(json!({ "command": "prepare_verified_input_target" }));
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "W07/W09 edge seed must provide the deterministic verified-input target fixture: {response}"
        );
        assert_eq!(
            response.get("target_automation_id").and_then(Value::as_str),
            Some(INPUT_TARGET_AUTOMATION_ID)
        );
        response
    }

    pub fn input_state(&mut self) -> Value {
        let response = self.command(json!({ "command": "get_verified_input_state" }));
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "verified-input oracle state must be independently readable: {response}"
        );
        response
    }

    pub fn steal_foreground(&mut self) -> Value {
        let response = self.command(json!({ "command": "steal_foreground" }));
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "W07 edge seed must provide deterministic secondary foreground window: {response}"
        );
        assert_eq!(
            response.get("thief_is_foreground").and_then(Value::as_bool),
            Some(true),
            "W07 must prove foreground theft before entering LocalView final dispatch boundary"
        );
        response
    }

    pub fn open_modal_blocker(&mut self) -> Value {
        let response = self.command(json!({ "command": "open_modal_blocker" }));
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "W11 edge seed must provide deterministic owned modal blocker: {response}"
        );
        assert_eq!(
            response.get("modal_is_open").and_then(Value::as_bool),
            Some(true),
            "W11 must prove the owned modal is visible before LocalView final dispatch boundary"
        );
        response
    }

    pub fn close_modal_blocker(&mut self) -> Value {
        let response = self.command(json!({ "command": "close_modal_blocker" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            response.get("modal_is_open").and_then(Value::as_bool),
            Some(false)
        );
        response
    }

    pub fn hold_shift(&mut self) -> Value {
        let response = self.command(json!({ "command": "hold_shift" }));
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "W09 edge seed must provide a test-owned real Shift modifier fixture: {response}"
        );
        assert_eq!(
            response.get("shift_down").and_then(Value::as_bool),
            Some(true),
            "W09 must establish the real conflicting modifier before LocalView dispatch"
        );
        self.shift_owned = true;
        response
    }

    pub fn release_shift(&mut self) -> Value {
        let response = self.command(json!({ "command": "release_shift" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(response.get("shift_down").and_then(Value::as_bool), Some(false));
        self.shift_owned = false;
        response
    }

    pub fn kill_and_wait(mut self) {
        if self.shift_owned {
            let _ = self.release_shift();
        }
        self.child
            .kill()
            .expect("terminate WPF edge seed process for restart authority test" );
        self.child
            .wait()
            .expect("wait for terminated WPF edge seed process" );
        self.shutdown = true;
    }

    pub fn shutdown(mut self) {
        if self.shift_owned {
            let _ = self.release_shift();
        }
        let response = self.command(json!({ "command": "shutdown" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for WPF edge seed shutdown");
        assert!(status.success(), "WPF edge seed must exit cleanly: {status}");
    }
}

impl Drop for EdgeSeedProcess {
    fn drop(&mut self) {
        if self.shift_owned {
            let response = self.command_inner(json!({ "command": "release_shift" }));
            if response
                .as_ref()
                .and_then(|value| value.get("shift_down"))
                .and_then(Value::as_bool)
                == Some(false)
            {
                self.shift_owned = false;
            }
        }
        if !self.shutdown {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub fn truth_u64(value: &Value, field: &str) -> u64 {
    value
        .get(field)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("verified-input oracle field {field} must be u64: {value}"))
}

pub fn truth_bool(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("verified-input oracle field {field} must be bool: {value}"))
}

pub fn spawn_worker() -> WindowsUiaWorker {
    WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
        snapshot_budget: SnapshotBudget {
            max_nodes: 128,
            max_depth: 12,
            max_properties: 2048,
        },
        command_timeout: Duration::from_secs(5),
    })
    .expect("spawn production Windows UIA worker for W07/W09/W11")
}

pub fn attach_and_snapshot(
    worker: &WindowsUiaWorker,
    seed: &EdgeSeedProcess,
    window_handle: u64,
    cut: &str,
) -> (
    WindowsUiaAttachment,
    std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision>,
) {
    let attachment = worker
        .attach(UserSelectedWindowTarget {
            native_window_handle: window_handle,
            expected_process_id: seed.process_id(),
            selection_nonce: Uuid::new_v4(),
        })
        .expect("attach exact W07/W09/W11 edge-seed target window");
    let snapshot = worker
        .snapshot(
            &attachment,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: cut.into(),
                surface_scope: "seed:windows-uia:verified-input".into(),
            },
        )
        .expect("observe exact verified-input target through shipping Windows UIA provider");
    (attachment, snapshot)
}

pub struct VerifiedInputAuthority {
    pub journal: ConsequentialJournal,
    pub journal_path: PathBuf,
    pub permit: DispatchExecutionPermit,
    pub request: WindowsUiaVerifiedInputRequest,
}

pub async fn mint_verified_input_authority(
    attachment: &WindowsUiaAttachment,
    snapshot: &localview_native_provider::NativeSemanticSnapshotRevision,
    element_ref: localview_protocol::ProviderElementRef,
) -> VerifiedInputAuthority {
    let journal_path = std::env::temp_dir().join(format!(
        "localview-v43-real-verified-input-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = ConsequentialJournal::open(&journal_path)
        .await
        .expect("open isolated consequential journal for real verified input seed");
    let action_id = Uuid::new_v4();
    let envelope = CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: action_id,
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:v43-real-input"),
            acting_principal_ref: PrincipalRef::from("principal:acting:v43-real-input"),
            authorization_revision: "authorization:v43-real-input:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:v43-real-input-effect".into()],
        },
    };
    journal
        .record_intent_admitted(envelope.clone())
        .await
        .expect("durably admit exact real verified-input intent");
    let authorization = journal
        .record_authorization(
            action_id,
            envelope.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .expect("authorize exact real verified-input intent");
    let prepared = journal
        .record_dispatch_prepared(
            action_id,
            DispatchPreparationReceipt {
                receipt_ref: format!("prepared:v43-real-input:{action_id}"),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
                provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
                target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            },
        )
        .await
        .expect("durably prepare exact real verified-input authority");
    let (_, capability) = prepared.into_parts();
    let permit = journal
        .begin_dispatch(capability)
        .await
        .expect("mint one live verified-input dispatch permit");
    let batch = WindowsVerifiedKeyboardBatch::new(vec![
        WindowsVerifiedKeyEvent {
            virtual_key: SPACE_VK,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: SPACE_VK,
            transition: WindowsKeyTransition::KeyUp,
        },
    ])
    .expect("construct bounded Space key batch");
    let request = WindowsUiaVerifiedInputRequest::from_execution_permit(
        &permit,
        snapshot.snapshot_cut_ref().into(),
        attachment.provider_incarnation_ref().clone(),
        attachment.target_incarnation_ref().clone(),
        element_ref,
        WindowsUiaDispatchContextRequirements {
            require_foreground_target: true,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        },
        batch,
    );
    VerifiedInputAuthority {
        journal,
        journal_path,
        permit,
        request,
    }
}

pub async fn abandon_authority(authority: VerifiedInputAuthority) {
    authority
        .journal
        .abandon_dispatch_execution(authority.permit)
        .await
        .expect("blocked real input attempt must consume only volatile execution authority");
    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);
}
