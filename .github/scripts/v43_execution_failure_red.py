from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"
text = path.read_text()

provider_anchor = '''    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));\n\n    let _ = std::fs::remove_file(path);\n}\n\n#[tokio::test]\nasync fn forged_provider_receipt_is_never_linearized_and_leaves_prepared_for_reconciliation()'''
provider_replacement = '''    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));\n    let observation = journal\n        .begin_postcondition_observation(action_id)\n        .await\n        .expect("provider failure must release the live execution grant for same-process reconciliation");\n    journal\n        .abandon_postcondition_observation(observation)\n        .await\n        .unwrap();\n\n    let _ = std::fs::remove_file(path);\n}\n\n#[tokio::test]\nasync fn forged_provider_receipt_is_never_linearized_and_leaves_prepared_for_reconciliation()'''
if provider_anchor not in text:
    raise SystemExit("provider failure anchor missing")
text = text.replace(provider_anchor, provider_replacement, 1)

forged_anchor = '''    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));\n\n    let entries = journal.entries_for(action_id).await;'''
forged_replacement = '''    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));\n    let observation = journal\n        .begin_postcondition_observation(action_id)\n        .await\n        .expect("forged provider receipt must release the live execution grant for same-process reconciliation");\n    journal\n        .abandon_postcondition_observation(observation)\n        .await\n        .unwrap();\n\n    let entries = journal.entries_for(action_id).await;'''
if forged_anchor not in text:
    raise SystemExit("forged receipt anchor missing")
text = text.replace(forged_anchor, forged_replacement, 1)
path.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)
subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_execution_failure_red.py", ".github/workflows/v43-execution-failure-red.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): require same-process reconciliation after execution failure"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
