from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/src/execution_arm.rs"
text = path.read_text()

old = '''    verify_armed_canonical_before_executor(bridge, journal, session_id, &armed).await?;\n\n    let action_id = armed.action_id;\n'''
new = '''    if let Err(error) =\n        verify_armed_canonical_before_executor(bridge, journal, session_id, &armed).await\n    {\n        journal\n            .abandon_dispatch_execution(armed.dispatch_permit)\n            .await\n            .map_err(|abandonment| {\n                WindowsUiaDispatchExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {\n                    stage: "pre_executor_revalidation_failed",\n                    message: abandonment.to_string(),\n                }\n            })?;\n        return Err(error);\n    }\n\n    let action_id = armed.action_id;\n'''
if old not in text:
    raise SystemExit("pre-executor anchor missing or already changed")
text = text.replace(old, new, 1)
path.write_text(text)

commands = [
    ["cargo", "fmt", "--all"],
    ["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"],
    ["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior", "stale_canonical_authority_before_executor_releases_live_execution_grant", "--", "--nocapture"],
    ["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_execution_abandonment"],
    ["git", "diff", "--check"],
]
for command in commands:
    subprocess.run(command, cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/src/execution_arm.rs"], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_preexecutor_abandon_green.py",
    ".github/workflows/v43-preexecutor-abandon-green.yml",
    ".github/workflows/v43-preexecutor-abandon-red-evidence.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "fix(v43): release execution authority on pre-executor rejection"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
