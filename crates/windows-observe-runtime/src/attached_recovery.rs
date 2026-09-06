use localview_live_bridge::{
    ConsequentialJournal, ConsequentialRecoveryDebtDisposition, ConsequentialRecoveryState,
};
use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use thiserror::Error;
use uuid::Uuid;

use crate::{WindowsObserveProvider, WindowsObserveRuntimeManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAttachedRecoveryPlanEntry {
    pub action_id: Uuid,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub disposition: ConsequentialRecoveryDebtDisposition,
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
}

/// Build a read-only recovery plan for the exact currently attached Windows UIA
/// provider/target incarnation.
///
/// The planner consumes replay-derived durable lineage and typed debt disposition
/// only. It never captures provider state, creates observation/dispatch authority,
/// invokes a verifier, accepts an executor, or mutates the journal.
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

    let entries = journal
        .recovery_bindings_for_attachment(
            session_id,
            &provider_incarnation_ref,
            &target_incarnation_ref,
        )
        .await
        .into_iter()
        .map(|binding| {
            let disposition = binding.recovery_state.recovery_debt_disposition();
            WindowsUiaAttachedRecoveryPlanEntry {
                action_id: binding.action_id,
                recovery_state: binding.recovery_state,
                latest_journal_sequence: binding.latest_journal_sequence,
                expected_postcondition_contract_refs: binding.expected_postcondition_contract_refs,
                disposition,
            }
        })
        .collect();

    Ok(WindowsUiaAttachedRecoveryPlan {
        session_id,
        provider_incarnation_ref,
        target_incarnation_ref,
        entries,
    })
}
