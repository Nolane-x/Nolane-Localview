from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
source = root / "crates/windows-observe-runtime/src/execution_arm.rs"
text = source.read_text()

error_anchor = '''    #[error("Windows UIA durable dispatch linearization append failed: {message}")]\n    JournalLinearizationFailed { message: String },\n'''
error_replacement = '''    #[error("Windows UIA execution authority abandonment failed after {stage}: {message}")]\n    ExecutionAuthorityAbandonmentFailed {\n        stage: &'static str,\n        message: String,\n    },\n    #[error("Windows UIA durable dispatch linearization append failed: {message}")]\n    JournalLinearizationFailed { message: String },\n'''
if error_anchor not in text:
    raise SystemExit("error anchor missing")
text = text.replace(error_anchor, error_replacement, 1)

provider_anchor = '''    let provider_receipt = executor\n        .execute(&request)\n        .await\n        .map_err(|error| WindowsUiaDispatchExecutionCoordinatorError::ProviderExecutionFailed {\n            message: error.to_string(),\n        })?;\n\n    if !provider_receipt_matches_request(&provider_receipt, &request) {\n        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptMismatch);\n    }\n    if provider_receipt.transport_result != TransportResult::DeliveredToExecutor {\n        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptTransportMismatch);\n    }\n'''
provider_replacement = '''    let provider_receipt = match executor.execute(&request).await {\n        Ok(receipt) => receipt,\n        Err(error) => {\n            let message = error.to_string();\n            journal\n                .abandon_dispatch_execution(armed.dispatch_permit)\n                .await\n                .map_err(|abandonment| {\n                    WindowsUiaDispatchExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {\n                        stage: "provider_execution_failed",\n                        message: abandonment.to_string(),\n                    }\n                })?;\n            return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderExecutionFailed {\n                message,\n            });\n        }\n    };\n\n    if !provider_receipt_matches_request(&provider_receipt, &request) {\n        journal\n            .abandon_dispatch_execution(armed.dispatch_permit)\n            .await\n            .map_err(|abandonment| {\n                WindowsUiaDispatchExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {\n                    stage: "provider_receipt_mismatch",\n                    message: abandonment.to_string(),\n                }\n            })?;\n        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptMismatch);\n    }\n    if provider_receipt.transport_result != TransportResult::DeliveredToExecutor {\n        journal\n            .abandon_dispatch_execution(armed.dispatch_permit)\n            .await\n            .map_err(|abandonment| {\n                WindowsUiaDispatchExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {\n                    stage: "provider_receipt_transport_mismatch",\n                    message: abandonment.to_string(),\n                }\n            })?;\n        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptTransportMismatch);\n    }\n'''
if provider_anchor not in text:
    raise SystemExit("provider execution anchor missing")
text = text.replace(provider_anchor, provider_replacement, 1)
source.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_execution_abandonment"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_capability"], cwd=root, check=True)
subprocess.run(["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/src/execution_arm.rs", "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs", "crates/live-bridge/src/consequential_journal.rs", "crates/live-bridge/tests/v43_dispatch_execution_abandonment.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_execution_failure_green.py", ".github/workflows/v43-execution-failure-green.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "fix(v43): release execution authority before reconciliation"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
