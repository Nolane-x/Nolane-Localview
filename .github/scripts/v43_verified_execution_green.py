from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]

verified = root / "crates/windows-observe-runtime/src/verified_execution.rs"
verified.write_text(r'''use std::error::Error as StdError;

use localview_live_bridge::{
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,
    ConsequentialPostconditionReconciliationReceipt, ConsequentialRecoveryState, LiveBridge,
    reconcile_consequential_postconditions,
};
use localview_native_provider::NativeSemanticSnapshotRevision;
use localview_protocol::{DispatchResult, SessionId, WorldOutcome};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    WindowsObserveProvider, WindowsObserveRuntimeManager, WindowsUiaDispatchExecutionCoordinatorError,
    WindowsUiaDispatchExecutionPermit, WindowsUiaDispatchExecutor, execute_armed_uia_dispatch,
};

/// Independent predicate authority for consequential postconditions.
///
/// The verifier receives only the durable action id, the exact expected contract
/// set from the admitted envelope, and the immutable semantic revision captured
/// from a journal-minted post-dispatch cut. It cannot choose action lineage,
/// observation cut, world outcome, or commit state.
pub trait WindowsUiaPostconditionVerifier: Send + Sync {
    type Error: StdError + Send + Sync + 'static;

    fn verify(
        &self,
        action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsUiaVerifiedExecutionOutcome {
    Committed {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        dispatch_journal_sequence: u64,
        reconciliation_journal_sequence: u64,
        commit_journal_sequence: u64,
    },
    KnownNotDispatched {
        action_id: Uuid,
        dispatch_result: DispatchResult,
        dispatch_journal_sequence: u64,
    },
    PostconditionNotVerified {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        dispatch_journal_sequence: u64,
        reconciliation_journal_sequence: u64,
    },
}

#[derive(Debug, Error)]
pub enum WindowsUiaVerifiedExecutionError {
    #[error(transparent)]
    Dispatch(#[from] WindowsUiaDispatchExecutionCoordinatorError),
    #[error("post-dispatch observation authority failed: {message}")]
    ObservationAuthority { message: String },
    #[error("post-dispatch runtime capture failed: {message}")]
    Capture { message: String },
    #[error("durable admitted envelope is missing for consequential action {action_id}")]
    AdmittedEnvelopeMissing { action_id: Uuid },
    #[error("durable admitted envelope no longer matches the verified execution session")]
    AdmittedEnvelopeMismatch,
    #[error("runtime post-dispatch snapshot does not match the exact journal observation receipt")]
    SnapshotBindingMismatch,
    #[error("postcondition verifier failed: {message}")]
    Verifier { message: String },
    #[error("typed consequential reconciliation failed: {message}")]
    Reconciliation { message: String },
    #[error("durable consequential commit failed: {message}")]
    Commit { message: String },
    #[error("unexpected recovery state after durable dispatch evidence: {state:?}")]
    UnexpectedRecoveryState {
        state: Option<ConsequentialRecoveryState>,
    },
}

/// Execute one armed consequential UIA action through world verification.
///
/// `DispatchedFull` is intentionally not a terminal success here. Any outcome
/// that may have reached the provider must pass a fresh causal observation,
/// independent typed predicate verification, and durable reconciliation before
/// `Committed` can be returned. Known-not-dispatched outcomes terminate without
/// invoking the verifier because no world-side postcondition needs proving.
pub async fn execute_armed_uia_dispatch_verified<P, E, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    executor: &E,
    verifier: &V,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaVerifiedExecutionError>
where
    P: WindowsObserveProvider,
    E: WindowsUiaDispatchExecutor,
    V: WindowsUiaPostconditionVerifier,
{
    let dispatch = execute_armed_uia_dispatch(bridge, journal, session_id, armed, executor).await?;
    let action_id = dispatch.provider_receipt.action_id;
    let dispatch_result = dispatch.provider_receipt.dispatch_result;
    let dispatch_journal_sequence = dispatch.journal_entry.journal_sequence;

    let state = journal.recovery_state(action_id).await;
    if state == Some(ConsequentialRecoveryState::KnownNotDispatched) {
        return Ok(WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            action_id,
            dispatch_result,
            dispatch_journal_sequence,
        });
    }
    if state != Some(ConsequentialRecoveryState::PossiblyDispatched) {
        return Err(WindowsUiaVerifiedExecutionError::UnexpectedRecoveryState { state });
    }

    let permit = journal
        .begin_postcondition_observation(action_id)
        .await
        .map_err(|error| WindowsUiaVerifiedExecutionError::ObservationAuthority {
            message: error.to_string(),
        })?;
    let capture = runtime
        .capture_postcondition_observation_with_snapshot(journal, permit)
        .await
        .map_err(|error| WindowsUiaVerifiedExecutionError::Capture {
            message: error.to_string(),
        })?;
    let observation = capture.observation_receipt();
    let snapshot = capture.snapshot();

    if snapshot.snapshot_cut_ref() != observation.snapshot_cut_ref()
        || snapshot.provider_incarnation_ref() != observation.provider_incarnation_ref()
        || snapshot.target_incarnation_ref() != observation.target_incarnation_ref()
    {
        return Err(WindowsUiaVerifiedExecutionError::SnapshotBindingMismatch);
    }

    let envelope = journal
        .entries_for(action_id)
        .await
        .into_iter()
        .find_map(|entry| match entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
            _ => None,
        })
        .ok_or(WindowsUiaVerifiedExecutionError::AdmittedEnvelopeMissing { action_id })?;
    if envelope.transport_action_id != action_id || envelope.session_id != session_id {
        return Err(WindowsUiaVerifiedExecutionError::AdmittedEnvelopeMismatch);
    }

    let evidence = verifier
        .verify(
            action_id,
            &envelope.metadata.expected_postcondition_contract_refs,
            snapshot.as_ref(),
        )
        .map_err(|error| WindowsUiaVerifiedExecutionError::Verifier {
            message: error.to_string(),
        })?;

    let reconciliation = reconcile_consequential_postconditions(
        bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            capture.into_observation_receipt(),
            evidence,
        ),
    )
    .await
    .map_err(|error| WindowsUiaVerifiedExecutionError::Reconciliation {
        message: error.to_string(),
    })?;

    let reconciliation_journal_sequence = reconciliation.journal_entry.journal_sequence;
    if reconciliation.world_outcome == WorldOutcome::VerifiedExpected
        && reconciliation.postconditions_verified
    {
        let commit = journal
            .record_committed(action_id)
            .await
            .map_err(|error| WindowsUiaVerifiedExecutionError::Commit {
                message: error.to_string(),
            })?;
        return Ok(WindowsUiaVerifiedExecutionOutcome::Committed {
            action_id,
            world_outcome: reconciliation.world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
            commit_journal_sequence: commit.journal_sequence,
        });
    }

    Ok(WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
        action_id,
        world_outcome: reconciliation.world_outcome,
        dispatch_journal_sequence,
        reconciliation_journal_sequence,
    })
}
''')

lib = root / "crates/windows-observe-runtime/src/lib.rs"
text = lib.read_text()
if "mod verified_execution;" not in text:
    text = text.replace("mod runtime_manager;\n", "mod runtime_manager;\nmod verified_execution;\n", 1)
if "pub use verified_execution::*;" not in text:
    text = text.replace("pub use runtime_manager::*;\n", "pub use runtime_manager::*;\npub use verified_execution::*;\n", 1)
lib.write_text(text)

runtime = root / "crates/windows-observe-runtime/src/runtime_manager.rs"
text = runtime.read_text()

insert_anchor = '''#[derive(Debug, Clone, PartialEq, Eq)]\npub struct WindowsObserveDrainOutcome {\n'''
if "pub struct WindowsPostconditionObservationCapture" not in text:
    capture_struct = r'''#[derive(Debug, Clone)]
pub struct WindowsPostconditionObservationCapture {
    observation_receipt: ConsequentialPostconditionObservationReceipt,
    snapshot: Arc<NativeSemanticSnapshotRevision>,
}

impl WindowsPostconditionObservationCapture {
    pub fn observation_receipt(&self) -> &ConsequentialPostconditionObservationReceipt {
        &self.observation_receipt
    }

    pub fn snapshot(&self) -> &Arc<NativeSemanticSnapshotRevision> {
        &self.snapshot
    }

    pub fn into_observation_receipt(self) -> ConsequentialPostconditionObservationReceipt {
        self.observation_receipt
    }
}

'''
    if insert_anchor not in text:
        raise SystemExit("capture struct anchor missing")
    text = text.replace(insert_anchor, capture_struct + insert_anchor, 1)

old_sig = '''    pub async fn capture_postcondition_observation(\n        &self,\n        journal: &ConsequentialJournal,\n        permit: ConsequentialPostconditionObservationPermit,\n    ) -> Result<ConsequentialPostconditionObservationReceipt, WindowsObserveRuntimeError> {\n        let _gate = self.operation_gate.lock().await;\n'''
new_sig = '''    pub async fn capture_postcondition_observation(\n        &self,\n        journal: &ConsequentialJournal,\n        permit: ConsequentialPostconditionObservationPermit,\n    ) -> Result<ConsequentialPostconditionObservationReceipt, WindowsObserveRuntimeError> {\n        Ok(self\n            .capture_postcondition_observation_with_snapshot(journal, permit)\n            .await?\n            .into_observation_receipt())\n    }\n\n    /// Capture the exact immutable snapshot together with its opaque journal\n    /// observation receipt so downstream verification cannot race a later\n    /// runtime reconciliation and accidentally inspect a different revision.\n    pub async fn capture_postcondition_observation_with_snapshot(\n        &self,\n        journal: &ConsequentialJournal,\n        permit: ConsequentialPostconditionObservationPermit,\n    ) -> Result<WindowsPostconditionObservationCapture, WindowsObserveRuntimeError> {\n        let _gate = self.operation_gate.lock().await;\n'''
if old_sig not in text:
    raise SystemExit("capture method signature anchor missing")
text = text.replace(old_sig, new_sig, 1)

old_tail = '''        self.update_reconciliation_snapshot(session_id, snapshot)\n            .await;\n        drop(reconciliation_reservation);\n\n        journal\n            .complete_postcondition_observation(permit, reconciliation_receipt)\n            .await\n            .map_err(\n                |error| WindowsObserveRuntimeError::PostconditionObservationAuthority {\n                    message: error.to_string(),\n                },\n            )\n    }\n'''
new_tail = '''        self.update_reconciliation_snapshot(session_id, snapshot.clone())\n            .await;\n        drop(reconciliation_reservation);\n\n        let observation_receipt = journal\n            .complete_postcondition_observation(permit, reconciliation_receipt)\n            .await\n            .map_err(\n                |error| WindowsObserveRuntimeError::PostconditionObservationAuthority {\n                    message: error.to_string(),\n                },\n            )?;\n        Ok(WindowsPostconditionObservationCapture {\n            observation_receipt,\n            snapshot,\n        })\n    }\n'''
if old_tail not in text:
    raise SystemExit("capture method tail anchor missing")
text = text.replace(old_tail, new_tail, 1)
runtime.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "postcondition_capture_contract"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_execution_abandonment"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add",
    "crates/windows-observe-runtime/src/verified_execution.rs",
    "crates/windows-observe-runtime/src/lib.rs",
    "crates/windows-observe-runtime/src/runtime_manager.rs",
    "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs",
    "crates/windows-observe-runtime/src/execution_arm.rs",
    "crates/live-bridge/src/consequential_journal.rs",
    "crates/live-bridge/tests/v43_dispatch_execution_abandonment.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_verified_execution_green.py", ".github/workflows/v43-verified-execution-green.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "feat(v43): verify consequential execution before commit"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
