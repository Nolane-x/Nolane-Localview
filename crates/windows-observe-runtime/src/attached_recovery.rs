use localview_live_bridge::{
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState,
};
use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use thiserror::Error;
use uuid::Uuid;

use crate::{WindowsObserveProvider, WindowsObserveRuntimeManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaAttachedRecoveryDisposition {
    NotDispatched,
    VerificationRequired,
    CommitOnly,
    CompensatedTerminal,
    CompensationFailed,
    HistoricalCommitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAttachedRecoveryPlanEntry {
    pub action_id: Uuid,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub disposition: WindowsUiaAttachedRecoveryDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAttachedRecoveryPlan {
    pub session_id: SessionId,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub entries: Vec<WindowsUiaAttachedRecoveryPlanEntry>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaAttachedRecoveryPlanError {
    #[error("Windows UIA recovery planning requires an attached session {session_id}")]
    NotAttached { session_id: SessionId },
    #[error("durable admitted envelope is missing for consequential action {action_id}")]
    AdmittedEnvelopeMissing { action_id: Uuid },
}

/// Build a read-only recovery plan for the exact currently attached Windows UIA
/// provider/target incarnation.
///
/// Planning never creates observation authority, invokes a provider, verifies a
/// postcondition, mutates the journal, or reconstructs dispatch authority. World-
/// dependent debt is classified as `VerificationRequired` so a later boundary can
/// require an explicit contract verifier before reconciliation.
pub async fn plan_attached_consequential_recovery<P: WindowsObserveProvider>(
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
) -> Result<WindowsUiaAttachedRecoveryPlan, WindowsUiaAttachedRecoveryPlanError> {
    let snapshot = runtime
        .current_semantic_snapshot(session_id)
        .await
        .ok_or(WindowsUiaAttachedRecoveryPlanError::NotAttached { session_id })?;
    let provider_incarnation_ref = snapshot.provider_incarnation_ref().clone();
    let target_incarnation_ref = snapshot.target_incarnation_ref().clone();

    let mut entries = Vec::new();
    for inventory_entry in journal.recovery_inventory().await {
        let envelope = journal
            .entries_for(inventory_entry.action_id)
            .await
            .into_iter()
            .find_map(|entry| match entry.transition {
                ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
                _ => None,
            })
            .ok_or(
                WindowsUiaAttachedRecoveryPlanError::AdmittedEnvelopeMissing {
                    action_id: inventory_entry.action_id,
                },
            )?;

        if envelope.session_id != session_id
            || envelope.metadata.provider_incarnation_ref != provider_incarnation_ref
            || envelope.metadata.target_incarnation_ref != target_incarnation_ref
        {
            continue;
        }

        let disposition = match inventory_entry.recovery_state {
            ConsequentialRecoveryState::Admitted
            | ConsequentialRecoveryState::AuthorizedNotDispatched
            | ConsequentialRecoveryState::KnownNotDispatched => {
                WindowsUiaAttachedRecoveryDisposition::NotDispatched
            }
            ConsequentialRecoveryState::DispatchPrepared
            | ConsequentialRecoveryState::PossiblyDispatched
            | ConsequentialRecoveryState::OutcomeObservedUnverified => {
                WindowsUiaAttachedRecoveryDisposition::VerificationRequired
            }
            ConsequentialRecoveryState::VerifiedUncommitted => {
                WindowsUiaAttachedRecoveryDisposition::CommitOnly
            }
            ConsequentialRecoveryState::Compensated => {
                WindowsUiaAttachedRecoveryDisposition::CompensatedTerminal
            }
            ConsequentialRecoveryState::CompensationFailed => {
                WindowsUiaAttachedRecoveryDisposition::CompensationFailed
            }
            ConsequentialRecoveryState::Committed => {
                WindowsUiaAttachedRecoveryDisposition::HistoricalCommitted
            }
        };

        entries.push(WindowsUiaAttachedRecoveryPlanEntry {
            action_id: inventory_entry.action_id,
            recovery_state: inventory_entry.recovery_state,
            latest_journal_sequence: inventory_entry.latest_journal_sequence,
            expected_postcondition_contract_refs: envelope
                .metadata
                .expected_postcondition_contract_refs,
            disposition,
        });
    }

    Ok(WindowsUiaAttachedRecoveryPlan {
        session_id,
        provider_incarnation_ref,
        target_incarnation_ref,
        entries,
    })
}
