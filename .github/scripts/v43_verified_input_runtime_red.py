from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:140]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")

# Extend the existing behavioral fixture rather than duplicating a second Windows
# authority world. This keeps the RED on the same journal/runtime semantics.
path = "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"
replace_once(
    path,
    "    WindowsUiaProviderExecutionRequest, WindowsUiaVerifiedExecutionOutcome,\n    arm_uia_dispatch_execution, execute_armed_uia_dispatch, execute_armed_uia_dispatch_verified,\n    prepare_uia_dispatch, recover_consequential_uia_action,\n",
    "    WindowsUiaProviderExecutionRequest, WindowsUiaVerifiedExecutionOutcome,\n    WindowsUiaVerifiedInputExecutionCoordinatorError, WindowsUiaVerifiedInputExecutor,\n    WindowsUiaVerifiedInputProviderReceipt, arm_uia_dispatch_execution,\n    execute_armed_uia_dispatch, execute_armed_uia_dispatch_verified,\n    execute_armed_uia_verified_input, execute_armed_uia_verified_input_verified,\n    prepare_uia_dispatch, recover_consequential_uia_action,\n",
)
replace_once(
    path,
    "    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,\n    WindowsUiaPattern, WindowsUiaPatternSupport,\n",
    "    WindowsInputInsertionClass, WindowsKeyTransition, WindowsKeyboardStateSnapshot,\n    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,\n    WindowsUiaPattern, WindowsUiaPatternSupport, WindowsUiaVerifiedInputRequest,\n    WindowsVerifiedInputBoundaryReceipt, WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch,\n",
)
anchor = "#[derive(Debug, Clone, Copy)]\nenum VerifierMode {\n"
addition = r'''#[derive(Debug, Clone, Copy)]
enum VerifiedInputExecutorMode {
    Full,
    Partial,
    ForgeDigest,
}

#[derive(Debug)]
struct FakeVerifiedInputExecutor {
    mode: VerifiedInputExecutorMode,
    calls: Mutex<usize>,
}

impl FakeVerifiedInputExecutor {
    fn new(mode: VerifiedInputExecutorMode) -> Self {
        Self {
            mode,
            calls: Mutex::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl WindowsUiaVerifiedInputExecutor for FakeVerifiedInputExecutor {
    type Error = FakeExecutorError;

    async fn execute_verified_input(
        &self,
        request: WindowsUiaVerifiedInputRequest,
    ) -> Result<WindowsUiaVerifiedInputProviderReceipt, Self::Error> {
        *self.calls.lock().unwrap() += 1;
        let requested = request.batch().len() as u32;
        let (inserted, insertion_class) = match self.mode {
            VerifiedInputExecutorMode::Full => (requested, WindowsInputInsertionClass::FullyInserted),
            VerifiedInputExecutorMode::Partial => (
                requested.saturating_sub(1).max(1),
                WindowsInputInsertionClass::PartialDispatchUnknownOutcome,
            ),
            VerifiedInputExecutorMode::ForgeDigest => {
                (requested, WindowsInputInsertionClass::FullyInserted)
            }
        };
        let batch_digest = if matches!(self.mode, VerifiedInputExecutorMode::ForgeDigest) {
            "sha256:forged".to_owned()
        } else {
            request.batch_digest().to_owned()
        };
        let boundary = WindowsVerifiedInputBoundaryReceipt {
            dispatch_context: WindowsUiaDispatchContextObservation {
                target_window_handle: 0x1020,
                target_process_id: 102,
                foreground_window_handle: Some(0x1020),
                foreground_process_id: Some(102),
                exact_element_focused: Some(true),
                modal_blocker_window_handle: None,
            },
            keyboard_state: WindowsKeyboardStateSnapshot {
                shift_down: false,
                control_down: false,
                alt_down: false,
                left_windows_down: false,
                right_windows_down: false,
                caps_lock_on: false,
                num_lock_on: false,
                scroll_lock_on: false,
                layout_identity: Some("test-layout".into()),
            },
            requested_event_count: requested,
            inserted_event_count: inserted,
            insertion_class,
            raw_error_code: None,
            reconciliation_required: insertion_class
                != WindowsInputInsertionClass::ZeroInsertedBlocked,
        };
        Ok(WindowsUiaVerifiedInputProviderReceipt {
            dispatch_attempt_ref: request.dispatch_attempt_ref(),
            action_id: request.action_id(),
            preparation_journal_sequence: request.preparation_journal_sequence(),
            preparation_receipt_ref: request.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref: request.element_ref().clone(),
            batch_digest,
            transport_result: TransportResult::DeliveredToExecutor,
            boundary,
        })
    }
}

fn verified_input_batch() -> WindowsVerifiedKeyboardBatch {
    WindowsVerifiedKeyboardBatch::new(vec![
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyUp,
        },
    ])
    .unwrap()
}

'''
replace_once(path, anchor, addition + anchor)

append = r'''

#[tokio::test]
async fn partial_verified_input_is_durably_possibly_dispatched_and_reconciliation_only() {
    let (bridge, journal, path, _provider, armed) =
        prepared_and_armed("verified-input-partial").await;
    let action_id = armed.action_id();
    let executor = FakeVerifiedInputExecutor::new(VerifiedInputExecutorMode::Partial);

    let result = execute_armed_uia_verified_input(
        &bridge,
        &journal,
        session(),
        armed,
        verified_input_batch(),
        &executor,
    )
    .await
    .unwrap();

    assert_eq!(executor.call_count(), 1);
    assert_eq!(result.provider_receipt.boundary.inserted_event_count, 1);
    assert_eq!(
        result.provider_receipt.boundary.insertion_class,
        WindowsInputInsertionClass::PartialDispatchUnknownOutcome
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));
    assert!(matches!(
        &result.journal_entry.transition,
        ConsequentialJournalTransition::DispatchLinearized { receipt }
            if receipt.dispatch_result == DispatchResult::DispatchedPartial
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn forged_verified_input_receipt_is_never_linearized_and_cannot_restore_retry() {
    let (bridge, journal, path, _provider, armed) =
        prepared_and_armed("verified-input-forged").await;
    let action_id = armed.action_id();
    let executor = FakeVerifiedInputExecutor::new(VerifiedInputExecutorMode::ForgeDigest);

    let error = execute_armed_uia_verified_input(
        &bridge,
        &journal,
        session(),
        armed,
        verified_input_batch(),
        &executor,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        WindowsUiaVerifiedInputExecutionCoordinatorError::ProviderReceiptMismatch
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));
    assert!(!journal.entries_for(action_id).await.iter().any(|entry| matches!(
        entry.transition,
        ConsequentialJournalTransition::DispatchLinearized { .. }
    )));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn full_verified_input_reaches_world_success_only_after_fresh_postcondition_verification() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-input-full-verified").await;
    let executor = FakeVerifiedInputExecutor::new(VerifiedInputExecutorMode::Full);
    let verifier = FakeVerifier::new(VerifierMode::Pass);

    let outcome = execute_armed_uia_verified_input_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        verified_input_batch(),
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert_eq!(executor.call_count(), 1);
    assert_eq!(verifier.call_count(), 1);
    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::Committed {
            world_outcome: WorldOutcome::VerifiedExpected,
            ..
        }
    ));

    let _ = std::fs::remove_file(path);
}
'''
p = Path(path)
text = p.read_text(encoding="utf-8")
if "partial_verified_input_is_durably_possibly_dispatched" in text:
    raise SystemExit("runtime RED tests already appended")
p.write_text(text + append, encoding="utf-8")

# Task 4 explicitly requires its own contract file. Keep it small and focused on
# the non-public request minting rule so behavior remains in the shared fixture.
contract = Path("crates/windows-observe-runtime/tests/verified_input_execution_contract.rs")
if contract.exists():
    raise SystemExit(f"unexpected pre-existing {contract}")
contract.write_text(r'''use std::fs;

use localview_windows_observe_runtime::{
    WindowsUiaVerifiedInputExecutionCoordinatorError, WindowsUiaVerifiedInputExecutor,
    WindowsUiaVerifiedInputProviderReceipt,
};

#[test]
fn runtime_owns_verified_input_request_minting_and_exact_binding() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let execution_arm = fs::read_to_string(format!("{manifest}/src/execution_arm.rs"))
        .expect("read execution_arm.rs");
    let runtime_manager = fs::read_to_string(format!("{manifest}/src/runtime_manager.rs"))
        .expect("read runtime_manager.rs");

    assert!(
        execution_arm.contains("WindowsUiaVerifiedInputRequest::from_execution_permit"),
        "runtime coordinator must mint the provider request from the hidden one-shot permit"
    );
    assert!(
        execution_arm.contains("verified_input_receipt_matches_request"),
        "runtime must compare exact provider receipt binding before journal linearization"
    );
    assert!(
        execution_arm.contains("record_dispatch_linearized"),
        "verified input must reuse the canonical consequential journal writer"
    );
    assert!(
        runtime_manager.contains("impl crate::WindowsUiaVerifiedInputExecutor for WindowsUiaRuntimeDispatchExecutor"),
        "production runtime executor must route verified input through the existing attached worker"
    );

    fn assert_error(error: WindowsUiaVerifiedInputExecutionCoordinatorError) {
        let _ = error;
    }
    fn assert_receipt(receipt: WindowsUiaVerifiedInputProviderReceipt) {
        let _ = receipt;
    }
    fn assert_executor<T: WindowsUiaVerifiedInputExecutor>() {}
    let _ = assert_error as fn(WindowsUiaVerifiedInputExecutionCoordinatorError);
    let _ = assert_receipt as fn(WindowsUiaVerifiedInputProviderReceipt);
    let _ = assert_executor::<localview_windows_observe_runtime::WindowsUiaRuntimeDispatchExecutor>;
}
''', encoding="utf-8")

print("Task 4 verified-input runtime RED staged")
