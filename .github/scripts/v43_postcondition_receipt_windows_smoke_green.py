from pathlib import Path
import os
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/windows_runtime_dispatch_smoke.rs"
text = path.read_text()

old_import = '''        ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,\n        ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n'''
new_import = '''        ActionEnvelopeMetadata, ActionIdempotencyClass, ActionPostconditionVerdict, ActionRiskClass,\n        BridgeActionKind, ConsequentialJournal, ConsequentialJournalTransition,\n        ConsequentialPostconditionEvidence,\n'''
if text.count(old_import) != 1:
    raise SystemExit(f"import anchor count={text.count(old_import)}")
text = text.replace(old_import, new_import, 1)

old = '''        let reconciliation_sequence = entries\n            .iter()\n            .find(|entry| {\n                matches!(\n                    entry.transition,\n                    ConsequentialJournalTransition::ReconciliationOutcome {\n                        world_outcome: WorldOutcome::VerifiedExpected,\n                        postconditions_verified: true,\n                        ..\n                    }\n                )\n            })\n            .map(|entry| entry.journal_sequence)\n            .expect("fresh postcondition evidence must be durably reconciled");\n        let commit_sequence = entries\n            .iter()\n            .find(|entry| matches!(entry.transition, ConsequentialJournalTransition::Committed))\n            .map(|entry| entry.journal_sequence)\n            .expect("verified expected world outcome must be durably committed");\n        assert!(dispatch_sequence < reconciliation_sequence);\n        assert!(reconciliation_sequence < commit_sequence);\n'''
new = '''        let postcondition_sequence = entries\n            .iter()\n            .find_map(|entry| match &entry.transition {\n                ConsequentialJournalTransition::PostconditionReceiptRecorded { receipt }\n                    if receipt.verdict == ActionPostconditionVerdict::VerifiedExpected =>\n                {\n                    assert_eq!(\n                        receipt.completion_journal_sequence,\n                        entry.journal_sequence,\n                        "journal must mint the receipt completion sequence"\n                    );\n                    assert_eq!(\n                        receipt.causal_assurance.causal_journal_sequence(),\n                        dispatch_sequence,\n                        "verified receipt must causally bind to the real dispatch"\n                    );\n                    Some(entry.journal_sequence)\n                }\n                _ => None,\n            })\n            .expect("fresh postcondition evidence must produce a durable verified receipt");\n        let commit_sequence = entries\n            .iter()\n            .find(|entry| matches!(entry.transition, ConsequentialJournalTransition::Committed))\n            .map(|entry| entry.journal_sequence)\n            .expect("verified expected world outcome must be durably committed");\n        assert!(dispatch_sequence < postcondition_sequence);\n        assert!(postcondition_sequence < commit_sequence);\n'''
if text.count(old) != 1:
    raise SystemExit(f"legacy smoke assertion anchor count={text.count(old)}")
text = text.replace(old, new, 1)
path.write_text(text)

subprocess.run(["rustfmt", "--edition", "2024", str(path)], cwd=root, check=True)
subprocess.run([
    "cargo", "test", "-p", "localview-live-bridge", "--test", "v43_action_postcondition_receipt", "--", "--nocapture"
], cwd=root, check=True)
env = os.environ.copy()
env["LOCALVIEW_UIA_SMOKE"] = "1"
subprocess.run([
    "cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "windows_runtime_dispatch_smoke", "--", "--ignored", "--nocapture", "--test-threads=1"
], cwd=root, env=env, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/tests/windows_runtime_dispatch_smoke.rs"], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_postcondition_receipt_windows_smoke_green.py",
    ".github/workflows/v43-postcondition-receipt-windows-smoke-green.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): assert durable postcondition receipt in real smoke"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-durable-action-postcondition-receipt"], cwd=root, check=True)
