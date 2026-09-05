from pathlib import Path
import subprocess

BRANCH = "feat/v43-consequential-postcondition-reconciliation-clean"
root = Path.cwd()

def run(*args):
    return subprocess.run(args, check=True, text=True, capture_output=True)

matches = run("git", "grep", "-n", "record_reconciliation_outcome", "--", "*.rs").stdout.strip().splitlines()
allowed = {
    "crates/live-bridge/src/consequential_journal.rs",
    "crates/live-bridge/src/postcondition_reconciliation.rs",
    "crates/live-bridge/tests/v43_consequential_journal.rs",
}
unexpected = []
for match in matches:
    path = match.split(":", 1)[0]
    if path not in allowed:
        unexpected.append(match)
if unexpected:
    raise SystemExit("unexpected raw reconciliation call sites:\n" + "\n".join(unexpected))

journal = root / "crates/live-bridge/src/consequential_journal.rs"
text = journal.read_text()
old = "    pub async fn record_reconciliation_outcome(\n"
new = "    pub(crate) async fn record_reconciliation_outcome(\n"
if new not in text:
    if old not in text:
        raise SystemExit("record_reconciliation_outcome visibility sentinel missing")
    text = text.replace(old, new, 1)
journal.write_text(text)

test = root / "crates/live-bridge/tests/v43_consequential_journal.rs"
text = test.read_text()
old_import = '''use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, ConsequentialRecoveryState,
    DispatchExecutionPermit, DispatchLinearizationReceipt, DispatchPreparationReceipt,
};
'''
new_import = '''use localview_live_bridge::{
    reconcile_consequential_postconditions, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalError,
    ConsequentialPostconditionEvidence, ConsequentialPostconditionReconciliationReceipt,
    ConsequentialPostconditionStatus, ConsequentialRecoveryState, DispatchExecutionPermit,
    DispatchLinearizationReceipt, DispatchPreparationReceipt, LiveBridge,
    ProviderObservationBinding,
};
'''
if new_import not in text:
    if old_import not in text:
        raise SystemExit("live-bridge test import sentinel missing")
    text = text.replace(old_import, new_import, 1)
old_protocol = '''use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
'''
new_protocol = '''use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderIncarnationRef,
    ReconciliationCompleteness, ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
'''
if new_protocol not in text:
    if old_protocol not in text:
        raise SystemExit("protocol test import sentinel missing")
    text = text.replace(old_protocol, new_protocol, 1)

helper_marker = '''async fn authorize_prepare_and_begin(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) -> DispatchExecutionPermit {
'''
helper = '''async fn record_typed_reconciliation(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    status: ConsequentialPostconditionStatus,
) {
    let bridge = LiveBridge::new(16, 4);
    bridge
        .bind_provider_observation(ProviderObservationBinding {
            session_id: action.session_id,
            generation: 1,
            provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
            target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            initial_continuity: EventContinuityState::OrderingOpaque,
            sequence_baseline: Some(0),
        })
        .await
        .unwrap();
    let snapshot_cut_ref = format!("cut:postcondition:{receipt_id}");
    assert!(
        bridge
            .record_reconciliation(
                action.session_id,
                ReconciliationSnapshotReceipt {
                    receipt_id: receipt_id.into(),
                    provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                    target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
                    snapshot_cut_ref: snapshot_cut_ref.clone(),
                    surface_scope: "journal-contract-fixture".into(),
                    completeness: ReconciliationCompleteness::Established,
                    cache_profile_revision: "cache:test:v1".into(),
                    permission_visibility_revision: "visibility:test:v1".into(),
                    capture_sequence: 1,
                    observed_digest: format!("digest:{receipt_id}"),
                    incompleteness_debt: Vec::new(),
                },
            )
            .await
    );
    reconcile_consequential_postconditions(
        &bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt {
            action_id: action.transport_action_id,
            provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
            target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            snapshot_cut_ref,
            reconciliation_receipt_ref: receipt_id.into(),
            postconditions: vec![ConsequentialPostconditionEvidence {
                contract_ref: "postcondition:message-visible".into(),
                status,
                receipt_ref: format!("postcondition:message-visible:{receipt_id}"),
            }],
        },
    )
    .await
    .unwrap();
}

'''
if "async fn record_typed_reconciliation(" not in text:
    if helper_marker not in text:
        raise SystemExit("test helper insertion sentinel missing")
    text = text.replace(helper_marker, helper + helper_marker, 1)

old_verified = '''    journal
        .record_reconciliation_outcome(
            action.transport_action_id,
            WorldOutcome::VerifiedExpected,
            Some("reconcile:1".into()),
            vec!["postcondition:message-visible:receipt".into()],
            true,
        )
        .await
        .unwrap();
'''
new_verified = '''    record_typed_reconciliation(
        &journal,
        &action,
        "reconcile:1",
        ConsequentialPostconditionStatus::VerifiedPass,
    )
    .await;
'''
if new_verified not in text:
    if old_verified not in text:
        raise SystemExit("verified raw reconciliation sentinel missing")
    text = text.replace(old_verified, new_verified, 1)

old_unexpected = '''    journal
        .record_reconciliation_outcome(
            action.transport_action_id,
            WorldOutcome::VerifiedUnexpected,
            Some("reconcile:unexpected".into()),
            Vec::new(),
            false,
        )
        .await
        .unwrap();
'''
new_unexpected = '''    record_typed_reconciliation(
        &journal,
        &action,
        "reconcile:unexpected",
        ConsequentialPostconditionStatus::VerifiedFail,
    )
    .await;
'''
if new_unexpected not in text:
    if old_unexpected not in text:
        raise SystemExit("unexpected raw reconciliation sentinel missing")
    text = text.replace(old_unexpected, new_unexpected, 1)

test.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], check=True)
subprocess.run(["cargo", "check", "-p", "localview-live-bridge", "--all-targets"], check=True)
for name in [
    "v43_consequential_journal",
    "v43_postcondition_reconciliation",
    "v43_postcondition_reconciliation_recovery",
]:
    subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", name, "--", "--nocapture"], check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], check=True)
subprocess.run([
    "git", "add",
    "crates/live-bridge/src/consequential_journal.rs",
    "crates/live-bridge/tests/v43_consequential_journal.rs",
], check=True)
subprocess.run([
    "git", "rm",
    ".github/workflows/v43-postcondition-authority-hardening.yml",
    ".github/scripts/v43_postcondition_authority_hardening.py",
], check=True)
subprocess.run([
    "git", "commit", "-m", "refactor(v43): seal raw postcondition journal authority"
], check=True)
subprocess.run(["git", "push", "origin", f"HEAD:{BRANCH}"], check=True)
