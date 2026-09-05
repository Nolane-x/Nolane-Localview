from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"
text = path.read_text()

old = '''    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n    ConsequentialPostconditionStatus, ConsequentialRecoveryState, LiveBridge,\n};\n'''
new = '''    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n    ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,\n    ConsequentialRecoveryState, LiveBridge, reconcile_consequential_postconditions,\n};\n'''
if text.count(old) != 1:
    raise SystemExit(f"live-bridge import anchor count={text.count(old)}")
text = text.replace(old, new, 1)

old = '''    WindowsUiaDispatchSealRequest, WindowsUiaPostconditionVerifier,\n    WindowsUiaPreparedDispatchRequest, WindowsUiaProviderExecutionReceipt,\n    WindowsUiaProviderExecutionRequest, WindowsUiaVerifiedExecutionOutcome,\n    arm_uia_dispatch_execution, execute_armed_uia_dispatch, execute_armed_uia_dispatch_verified,\n    prepare_uia_dispatch,\n};\n'''
new = '''    WindowsUiaConsequentialRecoveryOutcome, WindowsUiaDispatchSealRequest,\n    WindowsUiaPostconditionVerifier, WindowsUiaPreparedDispatchRequest,\n    WindowsUiaProviderExecutionReceipt, WindowsUiaProviderExecutionRequest,\n    WindowsUiaVerifiedExecutionOutcome, arm_uia_dispatch_execution, execute_armed_uia_dispatch,\n    execute_armed_uia_dispatch_verified, prepare_uia_dispatch, recover_consequential_uia_action,\n};\n'''
if text.count(old) != 1:
    raise SystemExit(f"runtime import anchor count={text.count(old)}")
text = text.replace(old, new, 1)

old = '''struct FakeProviderState {\n    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,\n    context_calls: usize,\n}\n'''
new = '''struct FakeProviderState {\n    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,\n    context_calls: usize,\n    snapshot_calls: usize,\n}\n'''
if text.count(old) != 1:
    raise SystemExit(f"provider state anchor count={text.count(old)}")
text = text.replace(old, new, 1)

old = '''    fn context_calls(&self) -> usize {\n        self.state.lock().unwrap().context_calls\n    }\n\n    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {\n'''
new = '''    fn context_calls(&self) -> usize {\n        self.state.lock().unwrap().context_calls\n    }\n\n    fn snapshot_call_count(&self) -> usize {\n        self.state.lock().unwrap().snapshot_calls\n    }\n\n    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {\n'''
if text.count(old) != 1:
    raise SystemExit(f"provider method anchor count={text.count(old)}")
text = text.replace(old, new, 1)

old = '''    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {\n        let snapshot = self.build_snapshot(snapshot_cut_ref);\n        self.state.lock().unwrap().snapshot = Some(snapshot.clone());\n        Ok(snapshot)\n    }\n'''
new = '''    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {\n        let snapshot = self.build_snapshot(snapshot_cut_ref);\n        let mut state = self.state.lock().unwrap();\n        state.snapshot_calls += 1;\n        state.snapshot = Some(snapshot.clone());\n        Ok(snapshot)\n    }\n'''
if text.count(old) != 1:
    raise SystemExit(f"snapshot implementation anchor count={text.count(old)}")
text = text.replace(old, new, 1)

append = r'''

async fn reconcile_once_without_commit(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<FakeProvider>,
    action_id: Uuid,
    verifier: &FakeVerifier,
) {
    let permit = journal
        .begin_postcondition_observation(action_id)
        .await
        .unwrap();
    let capture = runtime
        .capture_postcondition_observation_with_snapshot(journal, permit)
        .await
        .unwrap();
    let envelope = journal
        .entries_for(action_id)
        .await
        .into_iter()
        .find_map(|entry| match entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
            _ => None,
        })
        .unwrap();
    let evidence = verifier
        .verify(
            action_id,
            &envelope.metadata.expected_postcondition_contract_refs,
            capture.snapshot().as_ref(),
        )
        .unwrap();
    reconcile_consequential_postconditions(
        bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            capture.into_observation_receipt(),
            evidence,
        ),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn restart_after_prepared_without_dispatch_receipt_reconciles_without_redispatch() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("recovery-prepared-uncertain").await;
    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();
    let armed = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap();
    let action_id = armed.action_id();
    drop(armed);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    let snapshots_before = provider.snapshot_call_count();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let verifier = FakeVerifier::new(VerifierMode::Pass);
    let outcome = recover_consequential_uia_action(
        &bridge,
        &reopened,
        &runtime,
        action_id,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaConsequentialRecoveryOutcome::ReconciledCommitted { .. }
    ));
    assert_eq!(verifier.call_count(), 1);
    assert_eq!(provider.snapshot_call_count(), snapshots_before + 1);
    assert_eq!(
        reopened.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restart_after_dispatch_reconciles_fresh_world_without_any_executor_input() {
    let (bridge, journal, path, provider, runtime, armed) =
        verified_prepared_and_armed("recovery-possibly-dispatched").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap();
    assert_eq!(executor.call_count(), 1);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );
    let snapshots_before = provider.snapshot_call_count();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let verifier = FakeVerifier::new(VerifierMode::Pass);
    let outcome = recover_consequential_uia_action(
        &bridge,
        &reopened,
        &runtime,
        action_id,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaConsequentialRecoveryOutcome::ReconciledCommitted { .. }
    ));
    assert_eq!(verifier.call_count(), 1);
    assert_eq!(provider.snapshot_call_count(), snapshots_before + 1);
    assert_eq!(
        reopened.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restart_from_verified_uncommitted_is_commit_only_without_capture_or_verifier() {
    let (bridge, journal, path, provider, runtime, armed) =
        verified_prepared_and_armed("recovery-verified-uncommitted").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap();
    reconcile_once_without_commit(
        &bridge,
        &journal,
        &runtime,
        action_id,
        &FakeVerifier::new(VerifierMode::Pass),
    )
    .await;
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );
    let durable_receipt = journal
        .latest_action_postcondition_receipt(action_id)
        .await
        .unwrap();
    let snapshots_before = provider.snapshot_call_count();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let verifier = FakeVerifier::new(VerifierMode::Fail);
    let outcome = recover_consequential_uia_action(
        &bridge,
        &reopened,
        &runtime,
        action_id,
        &verifier,
    )
    .await
    .unwrap();

    match outcome {
        WindowsUiaConsequentialRecoveryOutcome::CommittedFromDurableReceipt {
            receipt_ref,
            receipt_journal_sequence,
            commit_journal_sequence,
            ..
        } => {
            assert_eq!(receipt_ref, durable_receipt.receipt_ref);
            assert_eq!(
                receipt_journal_sequence,
                durable_receipt.completion_journal_sequence
            );
            assert!(commit_journal_sequence > receipt_journal_sequence);
        }
        other => panic!("unexpected recovery outcome: {other:?}"),
    }
    assert_eq!(verifier.call_count(), 0);
    assert_eq!(provider.snapshot_call_count(), snapshots_before);
    assert_eq!(
        reopened.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restart_from_unknown_reobserves_fresh_cut_and_never_reopens_dispatch_authority() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("recovery-unknown-reobserve").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let unknown = FakeVerifier::new(VerifierMode::Unknown);
    let first = recover_consequential_uia_action(
        &bridge,
        &reopened,
        &runtime,
        action_id,
        &unknown,
    )
    .await
    .unwrap();
    assert!(matches!(
        first,
        WindowsUiaConsequentialRecoveryOutcome::PostconditionNotVerified {
            world_outcome: WorldOutcome::ReconciliationRequired,
            ..
        }
    ));
    assert_eq!(
        reopened.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert!(
        reopened
            .record_authorization(action_id, "authorization:retry-forbidden".into(), true)
            .await
            .is_err(),
        "unknown world outcome must never recreate dispatch authorization"
    );
    let first_cut = unknown.observed_cuts().into_iter().next().unwrap();
    drop(reopened);

    let reopened_again = ConsequentialJournal::open(&path).await.unwrap();
    let pass = FakeVerifier::new(VerifierMode::Pass);
    let second = recover_consequential_uia_action(
        &bridge,
        &reopened_again,
        &runtime,
        action_id,
        &pass,
    )
    .await
    .unwrap();
    assert!(matches!(
        second,
        WindowsUiaConsequentialRecoveryOutcome::ReconciledCommitted { .. }
    ));
    let second_cut = pass.observed_cuts().into_iter().next().unwrap();
    assert_ne!(first_cut, second_cut, "recovery must mint a fresh observation cut");
    assert_eq!(executor.call_count(), 1, "recovery must never redispatch");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn restart_from_committed_is_historical_terminal_without_capture_or_verifier() {
    let (bridge, journal, path, provider, runtime, armed) =
        verified_prepared_and_armed("recovery-committed-terminal").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let initial_verifier = FakeVerifier::new(VerifierMode::Pass);
    let initial = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &initial_verifier,
    )
    .await
    .unwrap();
    assert!(matches!(initial, WindowsUiaVerifiedExecutionOutcome::Committed { .. }));
    let durable_receipt = journal
        .latest_action_postcondition_receipt(action_id)
        .await
        .unwrap();
    let snapshots_before = provider.snapshot_call_count();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let verifier = FakeVerifier::new(VerifierMode::Fail);
    let outcome = recover_consequential_uia_action(
        &bridge,
        &reopened,
        &runtime,
        action_id,
        &verifier,
    )
    .await
    .unwrap();
    match outcome {
        WindowsUiaConsequentialRecoveryOutcome::AlreadyCommitted {
            receipt_ref,
            receipt_journal_sequence,
            ..
        } => {
            assert_eq!(receipt_ref, durable_receipt.receipt_ref);
            assert_eq!(
                receipt_journal_sequence,
                durable_receipt.completion_journal_sequence
            );
        }
        other => panic!("unexpected recovery outcome: {other:?}"),
    }
    assert_eq!(verifier.call_count(), 0);
    assert_eq!(provider.snapshot_call_count(), snapshots_before);
    let _ = std::fs::remove_file(path);
}
'''
if "restart_after_prepared_without_dispatch_receipt_reconciles_without_redispatch" in text:
    raise SystemExit("recovery RED tests already present")
text += append
path.write_text(text)
subprocess.run(["rustfmt", "--edition", "2024", str(path)], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", str(path.relative_to(root))], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_recovery_red_patcher.py",
    ".github/workflows/v43-recovery-red-patcher.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): define consequential restart recovery contract"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-recovery-coordinator"], cwd=root, check=True)
