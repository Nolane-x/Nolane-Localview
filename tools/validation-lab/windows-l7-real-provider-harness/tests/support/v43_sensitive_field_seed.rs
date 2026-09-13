#![cfg(windows)]

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};

use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
};
use serde_json::{Value, json};
use uuid::Uuid;

pub const SENSITIVE_FIELD_AUTOMATION_ID: &str = "LocalViewW13SensitiveField";

pub struct SensitiveEdgeSeedProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    shutdown: bool,
}

impl SensitiveEdgeSeedProcess {
    pub fn spawn() -> Self {
        let binary = std::env::var_os("LOCALVIEW_UIA_EDGE_SEED_BIN")
            .expect("LOCALVIEW_UIA_EDGE_SEED_BIN must point to the WPF edge seed executable");
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch isolated WPF UIA edge seed process for W13");
        let stdin = child.stdin.take().expect("edge seed stdin must be piped");
        let stdout = child.stdout.take().expect("edge seed stdout must be piped");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            shutdown: false,
        }
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    fn command(&mut self, command: Value) -> Value {
        serde_json::to_writer(&mut self.stdin, &command)
            .expect("serialize W13 oracle command");
        writeln!(self.stdin).expect("terminate W13 oracle command");
        self.stdin.flush().expect("flush W13 oracle command");

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read W13 oracle response");
        assert!(!line.trim().is_empty(), "W13 oracle must return one JSON line");
        serde_json::from_str(&line).expect("decode W13 oracle response")
    }

    pub fn prepare_sensitive_field(&mut self, canary: &str) -> Value {
        let response = self.command(json!({
            "command": "prepare_sensitive_field",
            "canary": canary,
        }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            response.get("target_automation_id").and_then(Value::as_str),
            Some(SENSITIVE_FIELD_AUTOMATION_ID)
        );
        assert_eq!(
            response.get("secret_length").and_then(Value::as_u64),
            Some(canary.len() as u64),
            "oracle may expose only secret length, never plaintext"
        );
        assert_eq!(
            response.get("value_read_count").and_then(Value::as_u64),
            Some(0),
            "W13 getter must begin unread"
        );
        assert_eq!(response.get("is_password").and_then(Value::as_bool), Some(true));
        response
    }

    pub fn sensitive_field_state(&mut self) -> Value {
        let response = self.command(json!({ "command": "get_sensitive_field_state" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        response
    }

    pub fn shutdown(mut self) {
        let response = self.command(json!({ "command": "shutdown" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for W13 edge seed shutdown");
        assert!(status.success(), "W13 edge seed must exit cleanly: {status}");
    }
}

impl Drop for SensitiveEdgeSeedProcess {
    fn drop(&mut self) {
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
        .unwrap_or_else(|| panic!("W13 oracle field {field} must be u64: {value}"))
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
    .expect("spawn production Windows UIA worker for W13")
}

pub fn attach_and_snapshot(
    worker: &WindowsUiaWorker,
    seed: &SensitiveEdgeSeedProcess,
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
        .expect("attach exact W13 edge-seed target window");
    let snapshot = worker
        .snapshot(
            &attachment,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: cut.into(),
                surface_scope: "seed:windows-uia:w13-sensitive-field".into(),
            },
        )
        .expect("observe W13 sensitive field through shipping Windows UIA provider");
    (attachment, snapshot)
}
