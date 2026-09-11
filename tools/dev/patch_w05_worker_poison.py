from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
lib_path = ROOT / "crates/windows-uia-provider/src/lib.rs"
root_path = ROOT / "crates/windows-uia-provider/src/event_buffer_lib.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


root = root_path.read_text(encoding="utf-8")
root = replace_once(
    root,
    "mod virtualized_item;\n#[path = \"lib.rs\"]\nmod worker;",
    "mod virtualized_item;\nmod worker_health;\n#[path = \"lib.rs\"]\nmod worker;",
    "worker_health module registration",
)
root_path.write_text(root, encoding="utf-8")

lib = lib_path.read_text(encoding="utf-8")
lib = replace_once(
    lib,
    "    #[error(\"Windows UI Automation provider command timed out\")]\n    CommandTimeout,\n",
    "    #[error(\"Windows UI Automation provider command timed out\")]\n    CommandTimeout,\n    #[error(\"Windows UI Automation worker is poisoned after a command timeout\")]\n    WorkerPoisoned,\n",
    "typed worker poison error",
)
lib = replace_once(
    lib,
    "    use super::*;\n    use crate::{\n",
    "    use super::*;\n    use crate::worker_health::{WorkerHealth, WorkerReceiveError};\n    use crate::{\n",
    "worker health import",
)
lib = replace_once(
    lib,
    "    pub struct WindowsUiaWorker {\n        sender: Sender<WorkerCommand>,\n        command_timeout: Duration,\n        provider_incarnation_ref: ProviderIncarnationRef,\n    }",
    "    pub struct WindowsUiaWorker {\n        sender: Sender<WorkerCommand>,\n        command_timeout: Duration,\n        provider_incarnation_ref: ProviderIncarnationRef,\n        health: Arc<WorkerHealth>,\n    }",
    "worker health field",
)
lib = replace_once(
    lib,
    "            Ok(Self {\n                sender: command_tx,\n                command_timeout: config.command_timeout,\n                provider_incarnation_ref,\n            })",
    "            Ok(Self {\n                sender: command_tx,\n                command_timeout: config.command_timeout,\n                provider_incarnation_ref,\n                health: Arc::new(WorkerHealth::new()),\n            })",
    "worker health initialization",
)
lib = replace_once(
    lib,
    "        pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {\n            &self.provider_incarnation_ref\n        }\n",
    "        fn ensure_healthy(&self) -> Result<(), WindowsUiaWorkerError> {\n            self.health\n                .ensure_healthy()\n                .map_err(|_| WindowsUiaWorkerError::WorkerPoisoned)\n        }\n\n        fn receive<T>(\n            &self,\n            receiver: &Receiver<Result<T, WindowsUiaWorkerError>>,\n        ) -> Result<T, WindowsUiaWorkerError> {\n            match self.health.recv_timeout(receiver, self.command_timeout) {\n                Ok(result) => result,\n                Err(WorkerReceiveError::Poisoned) => Err(WindowsUiaWorkerError::WorkerPoisoned),\n                Err(WorkerReceiveError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),\n                Err(WorkerReceiveError::Disconnected) => {\n                    Err(WindowsUiaWorkerError::WorkerUnavailable)\n                }\n            }\n        }\n\n        pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {\n            &self.provider_incarnation_ref\n        }\n",
    "worker health command helpers",
)

send_marker = "            let (reply_tx, reply_rx) = mpsc::channel();\n            self.sender\n"
send_count = lib.count(send_marker)
if send_count != 9:
    raise SystemExit(f"command pre-send health fence: expected 9 command sends, found {send_count}")
lib = lib.replace(
    send_marker,
    "            let (reply_tx, reply_rx) = mpsc::channel();\n            self.ensure_healthy()?;\n            self.sender\n",
)

recv_marker = "recv_command(reply_rx, self.command_timeout)"
recv_count = lib.count(recv_marker)
if recv_count != 9:
    raise SystemExit(f"command receive replacement: expected 9 receives, found {recv_count}")
lib = lib.replace(recv_marker, "self.receive(&reply_rx)")

old_recv = '''    fn recv_command<T>(
        receiver: Receiver<Result<T, WindowsUiaWorkerError>>,
        timeout: Duration,
    ) -> Result<T, WindowsUiaWorkerError> {
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),
            Err(RecvTimeoutError::Disconnected) => Err(WindowsUiaWorkerError::WorkerUnavailable),
        }
    }

'''
lib = replace_once(lib, old_recv, "", "legacy non-poisoning recv_command")
lib_path.write_text(lib, encoding="utf-8")
print("W05 primary-worker poison patch applied deterministically")
