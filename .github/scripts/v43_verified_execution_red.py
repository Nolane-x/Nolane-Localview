from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"
text = path.read_text()

text = text.replace(
'''    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,\n    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState, LiveBridge,\n''',
'''    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,\n    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n    ConsequentialPostconditionStatus, ConsequentialRecoveryState, LiveBridge,\n''', 1)
text = text.replace(
'''    ProviderIncarnationRef, ReconciliationCompleteness, SessionId, TargetIncarnationRef,\n    TransportResult,\n''',
'''    ProviderIncarnationRef, ReconciliationCompleteness, SessionId, TargetIncarnationRef,\n    TransportResult, WorldOutcome,\n''', 1)
text = text.replace(
'''    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutor,\n    WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchRequest,\n    WindowsUiaProviderExecutionReceipt, WindowsUiaProviderExecutionRequest,\n    arm_uia_dispatch_execution, execute_armed_uia_dispatch, prepare_uia_dispatch,\n''',
'''    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutor,\n    WindowsUiaDispatchSealRequest, WindowsUiaPostconditionVerifier,\n    WindowsUiaPreparedDispatchRequest, WindowsUiaProviderExecutionReceipt,\n    WindowsUiaProviderExecutionRequest, WindowsUiaVerifiedExecutionOutcome,\n    arm_uia_dispatch_execution, execute_armed_uia_dispatch,\n    execute_armed_uia_dispatch_verified, prepare_uia_dispatch,\n''', 1)

insert_before = '''fn session() -> SessionId {\n'''
verifier = r'''#[derive(Debug, Clone, Copy)]
enum VerifierMode {
    Pass,
    Unknown,
    Fail,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake postcondition verifier failure")]
struct FakeVerifierError;

#[derive(Debug)]
struct FakeVerifier {
    mode: VerifierMode,
    calls: Mutex<usize>,
    cuts: Mutex<Vec<String>>,
}

impl FakeVerifier {
    fn new(mode: VerifierMode) -> Self {
        Self {
            mode,
            calls: Mutex::new(0),
            cuts: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }

    fn observed_cuts(&self) -> Vec<String> {
        self.cuts.lock().unwrap().clone()
    }
}

impl WindowsUiaPostconditionVerifier for FakeVerifier {
    type Error = FakeVerifierError;

    fn verify(
        &self,
        _action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        *self.calls.lock().unwrap() += 1;
        self.cuts
            .lock()
            .unwrap()
            .push(snapshot.snapshot_cut_ref().to_owned());
        let status = match self.mode {
            VerifierMode::Pass => ConsequentialPostconditionStatus::VerifiedPass,
            VerifierMode::Unknown => ConsequentialPostconditionStatus::Unknown,
            VerifierMode::Fail => ConsequentialPostconditionStatus::VerifiedFail,
        };
        Ok(expected_contract_refs
            .iter()
            .map(|contract_ref| ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status,
                receipt_ref: format!(
                    "verifier:{}:{}",
                    snapshot.snapshot_cut_ref(),
                    contract_ref
                ),
            })
            .collect())
    }
}

'''
if insert_before not in text:
    raise SystemExit("session insertion anchor missing")
text = text.replace(insert_before, verifier + insert_before, 1)

helper_anchor = '''#[tokio::test]\nasync fn exact_provider_receipt_is_durably_linearized_before_returning_success() {\n'''
helper = r'''async fn verified_prepared_and_armed(
    label: &str,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    FakeProvider,
    WindowsObserveRuntimeManager<FakeProvider>,
    localview_windows_observe_runtime::WindowsUiaDispatchExecutionPermit,
) {
    let (bridge, journal, path, provider, runtime, seal_request) = fixture(label).await;
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
    (bridge, journal, path, provider, runtime, armed)
}

'''
if helper_anchor not in text:
    raise SystemExit("test helper insertion anchor missing")
text = text.replace(helper_anchor, helper + helper_anchor, 1)

append = r'''

#[tokio::test]
async fn verified_execution_commits_only_after_fresh_postdispatch_snapshot_passes() {
    let (bridge, journal, path, provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-pass").await;
    let action_id = armed.action_id();
    let pre_dispatch_cut = provider.snapshot().snapshot_cut_ref().to_owned();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Pass);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::Committed {
            world_outcome: WorldOutcome::VerifiedExpected,
            ..
        }
    ));
    assert_eq!(verifier.call_count(), 1);
    let cuts = verifier.observed_cuts();
    assert_eq!(cuts.len(), 1);
    assert_ne!(cuts[0], pre_dispatch_cut);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unknown_postcondition_never_becomes_committed_success() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-unknown").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Unknown);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            world_outcome: WorldOutcome::ReconciliationRequired,
            ..
        }
    ));
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert!(journal.entries_for(action_id).await.iter().all(|entry| !matches!(
        entry.transition,
        ConsequentialJournalTransition::Committed
    )));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn verified_failure_never_commits_expected_world_success() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-fail").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Fail);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            world_outcome: WorldOutcome::VerifiedUnexpected,
            ..
        }
    ));
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn known_not_dispatched_does_not_invoke_postcondition_verifier() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-not-dispatched").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::KnownNotDispatched);
    let verifier = FakeVerifier::new(VerifierMode::Pass);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            dispatch_result: DispatchResult::DispatchBlockedFocus,
            ..
        }
    ));
    assert_eq!(verifier.call_count(), 0);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::KnownNotDispatched)
    );

    let _ = std::fs::remove_file(path);
}
'''
if "verified_execution_commits_only_after_fresh_postdispatch_snapshot_passes" in text:
    raise SystemExit("verified execution tests already present")
text = text.rstrip() + append + "\n"
path.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)
subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_verified_execution_red.py", ".github/workflows/v43-verified-execution-red.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): require verified execution terminal outcomes"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
