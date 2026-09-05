from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/live-bridge/tests/v43_action_postcondition_receipt.rs"
text = path.read_text()
anchor = '''    assert_eq!(\n        receipt.evidence_receipt_refs,\n        vec!["evidence:visible:pass", "evidence:enabled:pass"]\n    );\n'''
insert = anchor + '''    assert_eq!(\n        receipt\n            .evidence_bindings\n            .iter()\n            .map(|binding| (binding.contract_ref.as_str(), binding.receipt_ref.as_str()))\n            .collect::<Vec<_>>(),\n        vec![\n            ("post:visible", "evidence:visible:pass"),\n            ("post:enabled", "evidence:enabled:pass"),\n        ]\n    );\n    let encoded = serde_json::to_value(receipt).unwrap();\n    assert_eq!(\n        encoded["causal_assurance"]["kind"],\n        serde_json::Value::String("dispatch_linearized".into())\n    );\n'''
if text.count(anchor) != 1:
    raise SystemExit(f"hardening anchor count={text.count(anchor)}")
path.write_text(text.replace(anchor, insert, 1))
subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)

completed = subprocess.run(
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_action_postcondition_receipt", "--", "--nocapture"],
    cwd=root,
    text=True,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
)
print(completed.stdout)
if completed.returncode == 0:
    raise SystemExit("expected hardening RED test to fail before production evidence bindings exist")
if "evidence_bindings" not in completed.stdout:
    raise SystemExit("RED failure did not mention the missing durable evidence_bindings authority")

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/live-bridge/tests/v43_action_postcondition_receipt.rs"], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_postcondition_receipt_hardening_red.py",
    ".github/workflows/v43-postcondition-receipt-hardening-red.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): bind durable postcondition evidence to contracts"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-durable-action-postcondition-receipt"], cwd=root, check=True)
