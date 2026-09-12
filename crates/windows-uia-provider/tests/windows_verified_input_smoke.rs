#![cfg(windows)]

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use localview_windows_uia_provider::{
    WindowsInputInsertRawResult, WindowsInputInsertionClass, WindowsKeyTransition,
    WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextObservation,
    WindowsUiaDispatchContextRequirements, WindowsVerifiedInputBoundaryError,
    WindowsVerifiedInputEnvironment, WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch,
    execute_windows_verified_input_boundary, observe_windows_verified_input_context,
    snapshot_windows_keyboard_state, windows_insert_verified_key_events,
};
use serde_json::{Value, json};

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
            .expect("launch isolated WPF verified-input smoke seed");
        let stdin = child.stdin.take().expect("edge seed stdin must be piped");
        let stdout = child.stdout.take().expect("edge seed stdout must be piped");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            shutdown: false,
        }
    }

    fn command(&mut self, command: Value) -> Value {
        serde_json::to_writer(&mut self.stdin, &command).expect("serialize edge-seed command");
        writeln!(self.stdin).expect("terminate edge-seed command JSON line");
        self.stdin.flush().expect("flush edge-seed command");
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read edge-seed JSON-line response");
        assert!(
            !line.trim().is_empty(),
            "edge seed closed its oracle channel unexpectedly"
        );
        serde_json::from_str(&line).expect("parse edge-seed JSON-line response")
    }

    fn prepare_input_target(&mut self) -> Value {
        let response = self.command(json!({ "command": "prepare_verified_input_target" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            response.get("target_is_foreground").and_then(Value::as_bool),
            Some(true),
            "production SendInput smoke must begin with the synthetic target foreground"
        );
        assert_eq!(
            response.get("effect_count").and_then(Value::as_u64),
            Some(0)
        );
        response
    }

    fn input_state(&mut self) -> Value {
        let response = self.command(json!({ "command": "get_verified_input_state" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        response
    }

    fn wait_for_input_effect(&mut self, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let state = self.input_state();
            if state
                .get("effect_count")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                > 0
            {
                return state;
            }
            assert!(
                Instant::now() < deadline,
                "SendInput reported full insertion but independent WPF target never observed Space"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn shutdown(mut self) {
        let response = self.command(json!({ "command": "shutdown" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for edge-seed shutdown");
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

struct ProductionWindowsEnvironment {
    target_window_handle: u64,
    target_process_id: u32,
    insert_calls: u32,
}

impl WindowsVerifiedInputEnvironment for ProductionWindowsEnvironment {
    fn observe_dispatch_context(
        &mut self,
    ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError> {
        observe_windows_verified_input_context(
            self.target_window_handle,
            self.target_process_id,
            true,
        )
    }

    fn snapshot_keyboard_state(
        &mut self,
    ) -> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError> {
        snapshot_windows_keyboard_state()
    }

    fn insert_events(&mut self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult {
        self.insert_calls += 1;
        windows_insert_verified_key_events(events)
    }
}

fn truth_u64(value: &Value, field: &str) -> u64 {
    value
        .get(field)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("edge-seed field {field} must be u64: {value}"))
}

#[test]
#[ignore = "requires interactive Windows desktop and LOCALVIEW_UIA_EDGE_SEED_BIN"]
fn production_send_input_full_dispatch_uses_verified_receipt_path() {
    assert!(
        std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
        "real production input smoke must be explicitly enabled"
    );

    let mut seed = EdgeSeedProcess::spawn();
    let fixture = seed.prepare_input_target();
    let target_window_handle = truth_u64(&fixture, "window_handle");
    let target_process_id = truth_u64(&fixture, "process_id") as u32;
    assert_ne!(target_window_handle, 0);
    assert_ne!(target_process_id, 0);

    let batch = WindowsVerifiedKeyboardBatch::new(vec![
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyUp,
        },
    ])
    .expect("construct bounded Space batch");
    let mut environment = ProductionWindowsEnvironment {
        target_window_handle,
        target_process_id,
        insert_calls: 0,
    };

    let receipt = execute_windows_verified_input_boundary(
        WindowsUiaDispatchContextRequirements {
            require_foreground_target: true,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        },
        &batch,
        &mut environment,
    )
    .expect("production Win32 backend must traverse the verified-input receipt boundary");

    assert_eq!(environment.insert_calls, 1, "SendInput must be attempted exactly once");
    assert_eq!(receipt.requested_event_count, 2);
    assert_eq!(receipt.inserted_event_count, 2);
    assert_eq!(
        receipt.insertion_class,
        WindowsInputInsertionClass::FullyInserted
    );
    assert!(
        receipt.reconciliation_required,
        "full platform acceptance still requires world reconciliation"
    );

    let after = seed.wait_for_input_effect(Duration::from_secs(2));
    assert_eq!(
        truth_u64(&after, "effect_count"),
        1,
        "independent WPF oracle must observe exactly one Space key effect"
    );
    assert_eq!(
        after.get("target_is_foreground").and_then(Value::as_bool),
        Some(true),
        "production smoke must not depend on changing foreground ownership"
    );

    seed.shutdown();
}
