use std::error::Error as StdError;

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
    WindowsObserveProvider, WindowsObserveRuntimeManager,
    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutionPermit,
    WindowsUiaDispatchExecutor, execute_armed_uia_dispatch,
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

/// Restart/recovery result for one consequential UIA action.
///
/// Recovery deliberately has no executor-bearing variant. Durable state may
/// authorize observation, verification, or commit, but it never reconstructs a
/// dispatch capability after process death.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsUiaConsequentialRecoveryOutcome {
    ReconciledCommitted {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        reconciliation_journal_sequence: u64,
        commit_journal_sequence: u64,
    },
    CommittedFromDurableReceipt {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        receipt_ref: String,
        receipt_journal_sequence: u64,
        commit_journal_sequence: u64,
    },
    PostconditionNotVerified {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        reconciliation_journal_sequence: u64,
    },
    AlreadyCommitted {
        action_id: Uuid,
        world_outcome: WorldOutcome,
        receipt_ref: String,
        receipt_journal_sequence: u64,
    },
    NotDispatched {
        action_id: Uuid,
        durable_state: ConsequentialRecoveryState,
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
    #[error("durable postcondition receipt is missing for consequential action {action_id}")]
    DurablePostconditionReceiptMissing { action_id: Uuid },
    #[error("durable postcondition receipt is not VerifiedExpected for consequential action {action_id}")]
    DurablePostconditionReceiptNotVerified { action_id: Uuid },
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
/// Pre-executor authority rejection is handled by the lower dispatch coordinator,
/// which releases only the process-local execution grant and preserves durable
/// PREPARED uncertainty for same-process reconciliation.
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
        .map_err(
            |error| WindowsUiaVerifiedExecutionError::ObservationAuthority {
                message: error.to_string(),
            },
        )?;
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
        let commit = journal.record_committed(action_id).await.map_err(|error| {
            WindowsUiaVerifiedExecutionError::Commit {
                message: error.to_string(),
            }
        })?;
        return Ok(WindowsUiaVerifiedExecutionOutcome::Committed {
            action_id,
            world_outcome: reconciliation.world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
            commit_journal_sequence: commit.journal_sequence,
        });
    }

    Ok(
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            action_id,
            world_outcome: reconciliation.world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
        },
    )
}

/// Recover one consequential UIA action using only durable journal authority.
///
/// This API intentionally accepts no dispatch executor or dispatch permit. A
/// reopened journal reconstructs no process-local dispatch grant, so PREPARED,
/// possibly-dispatched, and previously-unknown states can only move forward by
/// observing the current world and reconciling. `VerifiedUncommitted` is
/// commit-only from the durable VerifiedExpected receipt; `Committed` is a
/// historical terminal read with no provider capture or verifier invocation.
pub async fn recover_consequential_uia_action<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    action_id: Uuid,
    verifier: &V,
) -> Result<WindowsUiaConsequentialRecoveryOutcome, WindowsUiaVerifiedExecutionError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let state = journal.recovery_state(action_id).await;
    match state {
        Some(ConsequentialRecoveryState::Committed) => {
            let receipt = journal
                .latest_action_postcondition_receipt(action_id)
                .await
                .ok_or(
                    WindowsUiaVerifiedExecutionError::DurablePostconditionReceiptMissing {
                        action_id,
                    },
                )?;
            if receipt.verdict.world_outcome() != WorldOutcome::VerifiedExpected
                || !receipt.verdict.postconditions_verified()
            {
                return Err(
                    WindowsUiaVerifiedExecutionError::DurablePostconditionReceiptNotVerified {
                        action_id,
                    },
                );
            }
            return Ok(WindowsUiaConsequentialRecoveryOutcome::AlreadyCommitted {
                action_id,
                world_outcome: receipt.verdict.world_outcome(),
                receipt_ref: receipt.receipt_ref,
                receipt_journal_sequence: receipt.completion_journal_sequence,
            });
        }
        Some(ConsequentialRecoveryState::VerifiedUncommitted) => {
            let receipt = journal
                .latest_action_postcondition_receipt(action_id)
                .await
                .ok_or(
                    WindowsUiaVerifiedExecutionError::DurablePostconditionReceiptMissing {
                        action_id,
                    },
                )?;
            if receipt.verdict.world_outcome() != WorldOutcome::VerifiedExpected
                || !receipt.verdict.postconditions_verified()
            {
                return Err(
                    WindowsUiaVerifiedExecutionError::DurablePostconditionReceiptNotVerified {
                        action_id,
                    },
                );
            }
            let receipt_ref = receipt.receipt_ref;
            let receipt_journal_sequence = receipt.completion_journal_sequence;
            let world_outcome = receipt.verdict.world_outcome();
            let commit = journal.record_committed(action_id).await.map_err(|error| {
                WindowsUiaVerifiedExecutionError::Commit {
                    message: error.to_string(),
                }
            })?;
            return Ok(
                WindowsUiaConsequentialRecoveryOutcome::CommittedFromDurableReceipt {
                    action_id,
                    world_outcome,
                    receipt_ref,
                    receipt_journal_sequence,
                    commit_journal_sequence: commit.journal_sequence,
                },
            );
        }
        Some(
            durable_state @ (ConsequentialRecoveryState::Admitted
            | ConsequentialRecoveryState::AuthorizedNotDispatched
            | ConsequentialRecoveryState::KnownNotDispatched),
        ) => {
            return Ok(WindowsUiaConsequentialRecoveryOutcome::NotDispatched {
                action_id,
                durable_state,
            });
        }
        Some(ConsequentialRecoveryState::DispatchPrepared)
        | Some(ConsequentialRecoveryState::PossiblyDispatched)
        | Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => {}
        other => {
            return Err(WindowsUiaVerifiedExecutionError::UnexpectedRecoveryState {
                state: other,
            });
        }
    }

    let permit = journal
        .begin_postcondition_observation(action_id)
        .await
        .map_err(
            |error| WindowsUiaVerifiedExecutionError::ObservationAuthority {
                message: error.to_string(),
            },
        )?;
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
    if envelope.transport_action_id != action_id
        || envelope.session_id != observation.session_id()
        || envelope.metadata.provider_incarnation_ref != *observation.provider_incarnation_ref()
        || envelope.metadata.target_incarnation_ref != *observation.target_incarnation_ref()
    {
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
        let commit = journal.record_committed(action_id).await.map_err(|error| {
            WindowsUiaVerifiedExecutionError::Commit {
                message: error.to_string(),
            }
        })?;
        return Ok(WindowsUiaConsequentialRecoveryOutcome::ReconciledCommitted {
            action_id,
            world_outcome: reconciliation.world_outcome,
            reconciliation_journal_sequence,
            commit_journal_sequence: commit.journal_sequence,
        });
    }

    Ok(
        WindowsUiaConsequentialRecoveryOutcome::PostconditionNotVerified {
            action_id,
            world_outcome: reconciliation.world_outcome,
            reconciliation_journal_sequence,
        },
    )
}
