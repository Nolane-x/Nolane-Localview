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

pub const WEAK_ACCESSIBILITY_AUTOMATION_ID: &str = "LocalViewW14WeakAccessibility";

pub struct WeakAccessibilityEdgeSeedProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    shutdown: bool,
}

impl WeakAccessibilityEdgeSeedProcess {
    pub fn spawn() -> Self {
        let binary = std::env::var_os("LOCALVIEW_UIA_EDGE_SEED_BIN")
            .expect("LOCALVIEW_UIA_EDGE_SEED_BIN must point to the WPF edge seed executable");
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch isolated WPF UIA edge seed process for W14");
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
            .expect("serialize W14 oracle command");
        writeln!(self.stdin).expect("terminate W14 oracle command");
        self.stdin.flush().expect("flush W14 oracle command");

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read W14 oracle response");
        assert!(!line.trim().is_empty(), "W14 oracle must return one JSON line");
        serde_json::from_str(&line).expect("decode W14 oracle response")
    }

    pub fn prepare_weak_accessibility(&mut self) -> Value {
        let response = self.command(json!({ "command": "prepare_weak_accessibility" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            response.get("target_automation_id").and_then(Value::as_str),
            Some(WEAK_ACCESSIBILITY_AUTOMATION_ID)
        );
        assert_eq!(response.get("owner_drawn").and_then(Value::as_bool), Some(true));
        assert_eq!(
            response.get("visual_effect_count").and_then(Value::as_u64),
            Some(0),
            "W14 owner-drawn visual action must begin without side effects"
        );
        response
    }

    pub fn weak_accessibility_state(&mut self) -> Value {
        let response = self.command(json!({ "command": "get_weak_accessibility_state" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        response
    }

    pub fn shutdown(mut self) {
        let response = self.command(json!({ "command": "shutdown" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for W14 edge seed shutdown");
        assert!(status.success(), "W14 edge seed must exit cleanly: {status}");
    }
}

impl Drop for WeakAccessibilityEdgeSeedProcess {
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
        .unwrap_or_else(|| panic!("W14 oracle field {field} must be u64: {value}"))
}

pub fn truth_str<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("W14 oracle field {field} must be string: {value}"))
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
    .expect("spawn production Windows UIA worker for W14")
}

pub fn attach_and_snapshot(
    worker: &WindowsUiaWorker,
    seed: &WeakAccessibilityEdgeSeedProcess,
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
        .expect("attach exact W14 edge-seed target window");
    let snapshot = worker
        .snapshot(
            &attachment,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref: cut.into(),
                surface_scope: "seed:windows-uia:w14-weak-accessibility".into(),
            },
        )
        .expect("observe W14 owner-drawn control through shipping Windows UIA provider");
    (attachment, snapshot)
}
