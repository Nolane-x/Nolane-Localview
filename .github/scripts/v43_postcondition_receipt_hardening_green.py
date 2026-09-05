from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
journal = root / "crates/live-bridge/src/consequential_journal.rs"
reconcile = root / "crates/live-bridge/src/postcondition_reconciliation.rs"
test = root / "crates/live-bridge/tests/v43_action_postcondition_receipt.rs"

journal_text = journal.read_text()

anchor = '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct ActionPostconditionReceipt {\n'''
insert = '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct ActionPostconditionEvidenceBinding {\n    pub contract_ref: String,\n    pub receipt_ref: String,\n}\n\n#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct ActionPostconditionReceipt {\n'''
if journal_text.count(anchor) != 1:
    raise SystemExit(f"receipt type anchor count={journal_text.count(anchor)}")
journal_text = journal_text.replace(anchor, insert, 1)

anchor = '''    pub reconciliation_receipt_ref: String,\n    pub evidence_receipt_refs: Vec<String>,\n    pub verified_contract_refs: Vec<String>,\n'''
insert = '''    pub reconciliation_receipt_ref: String,\n    /// Ordered evidence receipt summary. Receipt refs may repeat when one\n    /// evidence artifact proves multiple declared postcondition contracts.\n    pub evidence_receipt_refs: Vec<String>,\n    /// Exact contract-to-evidence provenance. A contract appears at most once;\n    /// verified/failed contracts always have a binding, while an unresolved\n    /// unknown may have no evidence artifact at all.\n    pub evidence_bindings: Vec<ActionPostconditionEvidenceBinding>,\n    pub verified_contract_refs: Vec<String>,\n'''
if journal_text.count(anchor) != 2:
    raise SystemExit(f"receipt/draft evidence anchor count={journal_text.count(anchor)}")
journal_text = journal_text.replace(anchor, insert, 2)

anchor = '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub enum ConsequentialPostconditionObservationCause {\n'''
insert = '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\n#[serde(tag = "kind", rename_all = "snake_case")]\npub enum ConsequentialPostconditionObservationCause {\n'''
if journal_text.count(anchor) != 1:
    raise SystemExit(f"cause serde anchor count={journal_text.count(anchor)}")
journal_text = journal_text.replace(anchor, insert, 1)

anchor = '''            reconciliation_receipt_ref: draft.reconciliation_receipt_ref,\n            evidence_receipt_refs: draft.evidence_receipt_refs,\n            verified_contract_refs: draft.verified_contract_refs,\n'''
insert = '''            reconciliation_receipt_ref: draft.reconciliation_receipt_ref,\n            evidence_receipt_refs: draft.evidence_receipt_refs,\n            evidence_bindings: draft.evidence_bindings,\n            verified_contract_refs: draft.verified_contract_refs,\n'''
if journal_text.count(anchor) != 1:
    raise SystemExit(f"receipt construction anchor count={journal_text.count(anchor)}")
journal_text = journal_text.replace(anchor, insert, 1)

old = '''    let mut evidence_refs = BTreeSet::new();\n    for evidence_ref in &receipt.evidence_receipt_refs {\n        if evidence_ref.trim().is_empty() || !evidence_refs.insert(evidence_ref.clone()) {\n            return Err(invalid("postcondition_receipt_invalid_evidence_refs"));\n        }\n    }\n    if receipt.evidence_receipt_refs.len() > expected.len()\n        || receipt.evidence_receipt_refs.len()\n            < receipt.verified_contract_refs.len() + receipt.failed_contract_refs.len()\n    {\n        return Err(invalid(\n            "postcondition_receipt_evidence_cardinality_mismatch",\n        ));\n    }\n\n    Ok(())\n'''
new = '''    let mut bound_contracts = BTreeSet::new();\n    let mut bound_receipt_refs = Vec::with_capacity(receipt.evidence_bindings.len());\n    for binding in &receipt.evidence_bindings {\n        if !expected.contains(&binding.contract_ref)\n            || !bound_contracts.insert(binding.contract_ref.clone())\n            || binding.receipt_ref.trim().is_empty()\n        {\n            return Err(invalid("postcondition_receipt_invalid_evidence_binding"));\n        }\n        bound_receipt_refs.push(binding.receipt_ref.clone());\n    }\n    if bound_receipt_refs != receipt.evidence_receipt_refs {\n        return Err(invalid("postcondition_receipt_evidence_summary_mismatch"));\n    }\n    if receipt\n        .verified_contract_refs\n        .iter()\n        .chain(receipt.failed_contract_refs.iter())\n        .any(|contract_ref| !bound_contracts.contains(contract_ref))\n    {\n        return Err(invalid(\n            "postcondition_receipt_decisive_contract_without_evidence",\n        ));\n    }\n\n    Ok(())\n'''
if journal_text.count(old) != 1:
    raise SystemExit(f"validation evidence block count={journal_text.count(old)}")
journal_text = journal_text.replace(old, new, 1)
journal.write_text(journal_text)

reconcile_text = reconcile.read_text()
anchor = '''    CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalEntry,\n    ConsequentialJournalError, ConsequentialJournalTransition,\n'''
insert = '''    ActionPostconditionEvidenceBinding, CanonicalActionEnvelope, ConsequentialJournal,\n    ConsequentialJournalEntry, ConsequentialJournalError, ConsequentialJournalTransition,\n'''
if reconcile_text.count(anchor) != 1:
    raise SystemExit(f"reconcile import anchor count={reconcile_text.count(anchor)}")
reconcile_text = reconcile_text.replace(anchor, insert, 1)

old = '''    let evidence_receipt_refs = expected_order\n        .iter()\n        .filter_map(|contract_ref| observed.get(contract_ref))\n        .map(|evidence| evidence.receipt_ref.clone())\n        .collect::<Vec<_>>();\n\n    let verdict = if !failed_contract_refs.is_empty() {\n'''
new = '''    let evidence_bindings = expected_order\n        .iter()\n        .filter_map(|contract_ref| {\n            observed\n                .get(contract_ref)\n                .map(|evidence| ActionPostconditionEvidenceBinding {\n                    contract_ref: contract_ref.clone(),\n                    receipt_ref: evidence.receipt_ref.clone(),\n                })\n        })\n        .collect::<Vec<_>>();\n    let evidence_receipt_refs = evidence_bindings\n        .iter()\n        .map(|binding| binding.receipt_ref.clone())\n        .collect::<Vec<_>>();\n\n    let verdict = if !failed_contract_refs.is_empty() {\n'''
if reconcile_text.count(old) != 1:
    raise SystemExit(f"reconcile evidence build block count={reconcile_text.count(old)}")
reconcile_text = reconcile_text.replace(old, new, 1)

anchor = '''            reconciliation_receipt_ref: observation.reconciliation_receipt_ref().to_owned(),\n            evidence_receipt_refs,\n            verified_contract_refs,\n'''
insert = '''            reconciliation_receipt_ref: observation.reconciliation_receipt_ref().to_owned(),\n            evidence_receipt_refs,\n            evidence_bindings,\n            verified_contract_refs,\n'''
if reconcile_text.count(anchor) != 1:
    raise SystemExit(f"reconcile draft anchor count={reconcile_text.count(anchor)}")
reconcile_text = reconcile_text.replace(anchor, insert, 1)
reconcile.write_text(reconcile_text)

test_text = test.read_text()
# Prove one durable evidence artifact may support multiple declared predicates.
test_text = test_text.replace('''                "evidence:enabled:pass",\n            ),\n''', '''                "evidence:visible:pass",\n            ),\n''', 1)
test_text = test_text.replace('''        vec!["evidence:visible:pass", "evidence:enabled:pass"]\n''', '''        vec!["evidence:visible:pass", "evidence:visible:pass"]\n''', 1)
test_text = test_text.replace('''            ("post:enabled", "evidence:enabled:pass"),\n''', '''            ("post:enabled", "evidence:visible:pass"),\n''', 1)
test.write_text(test_text)

for path in [journal, reconcile, test]:
    subprocess.run(["rustfmt", "--edition", "2024", str(path)], cwd=root, check=True)

commands = [
    ["cargo", "check", "-p", "localview-live-bridge", "--all-targets"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_action_postcondition_receipt", "--", "--nocapture"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation", "--", "--nocapture"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation_recovery", "--", "--nocapture"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_consequential_journal", "--", "--nocapture"],
    ["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"],
    ["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior", "--", "--nocapture"],
    ["git", "diff", "--check"],
]
for command in commands:
    subprocess.run(command, cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run([
    "git", "add",
    "crates/live-bridge/src/consequential_journal.rs",
    "crates/live-bridge/src/postcondition_reconciliation.rs",
    "crates/live-bridge/tests/v43_action_postcondition_receipt.rs",
], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_postcondition_receipt_hardening_green.py",
    ".github/workflows/v43-postcondition-receipt-hardening-green.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "feat(v43): bind postcondition evidence to contracts"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-durable-action-postcondition-receipt"], cwd=root, check=True)
