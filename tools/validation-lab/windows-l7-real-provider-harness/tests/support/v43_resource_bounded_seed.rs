#![cfg(windows)]

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use localview_native_provider::UserSelectedWindowTarget;
use serde_json::{Value, json};
use uuid::Uuid;

pub struct ResourceBoundedEdgeSeedProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    shutdown: bool,
}

impl ResourceBoundedEdgeSeedProcess {
    pub fn spawn() -> Self {
        let binary = std::env::var_os("LOCALVIEW_UIA_EDGE_SEED_BIN")
            .expect("LOCALVIEW_UIA_EDGE_SEED_BIN must point to the WPF edge seed executable");
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch isolated WPF UIA edge seed process for W15");
        let stdin = child.stdin.take().expect("W15 edge seed stdin must be piped");
        let stdout = child.stdout.take().expect("W15 edge seed stdout must be piped");
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
            .expect("serialize W15 edge seed oracle command");
        writeln!(self.stdin).expect("terminate W15 edge seed oracle command");
        self.stdin.flush().expect("flush W15 edge seed oracle command");

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read W15 edge seed oracle response");
        assert!(
            !line.trim().is_empty(),
            "W15 edge seed closed its oracle channel unexpectedly"
        );
        serde_json::from_str(&line).expect("decode W15 edge seed oracle response")
    }

    pub fn ground_truth(&mut self) -> Value {
        let response = self.command(json!({ "command": "get_ground_truth" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        response
    }

    pub fn selection(&self, window_handle: u64) -> UserSelectedWindowTarget {
        UserSelectedWindowTarget {
            native_window_handle: window_handle,
            expected_process_id: self.process_id(),
            selection_nonce: Uuid::new_v4(),
        }
    }

    pub fn shutdown(mut self) {
        let response = self.command(json!({ "command": "shutdown" }));
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true));
        self.shutdown = true;
        let status = self.child.wait().expect("wait for W15 edge seed shutdown");
        assert!(status.success(), "W15 edge seed must exit cleanly: {status}");
    }
}

impl Drop for ResourceBoundedEdgeSeedProcess {
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
        .unwrap_or_else(|| panic!("W15 oracle field {field} must be u64: {value}"))
}
