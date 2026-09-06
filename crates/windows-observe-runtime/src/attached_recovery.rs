use localview_live_bridge::{
    ConsequentialJournal, ConsequentialRecoveryActionScope, ConsequentialRecoveryDebtDisposition,
    ConsequentialRecoveryState, LiveBridge,
};
use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    recover_consequential_uia_action, WindowsObserveProvider, WindowsObserveRuntimeManager,
    WindowsUiaConsequentialRecoveryOutcome, WindowsUiaPostconditionVerifier,
    WindowsUiaVerifiedExecutionError,
};

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

/// One attachment-bound recovery result in the exact monotonic order returned by
/// the durable recovery plan.
///
/// Report-only variants deliberately carry no executor, dispatch permit, or
/// provider capability. They describe durable state without creating authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsUiaAttachedRecoveryDrainOutcome {
    NoDispatchProven {
        action_id: Uuid,
        durable_state: ConsequentialRecoveryState,
    },
    Recovered(WindowsUiaConsequentialRecoveryOutcome),
    HistoricalTerminal {
        action_id: Uuid,
        durable_state: ConsequentialRecoveryState,
    },
    ReconciliationRequired {
        action_id: Uuid,
        durable_state: ConsequentialRecoveryState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAttachedRecoveryDrain {
    pub session_id: SessionId,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub entries: Vec<WindowsUiaAttachedRecoveryDrainOutcome>,
}

#[derive(Debug, Error)]
pub enum WindowsUiaAttachedRecoveryDrainError {
    #[error(transparent)]
    Plan(#[from] WindowsUiaAttachedRecoveryPlanError),
    #[error("Windows UIA consequential recovery failed for action {action_id}: {source}")]
    Recovery {
        action_id: Uuid,
        #[source]
        source: WindowsUiaVerifiedExecutionError,
    },
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
    plan_attached_consequential_recovery_filtered(journal, runtime, session_id, |_| true).await
}

/// Build the same exact attachment-bound plan, but admit only action identities
/// captured by a caller-owned immutable recovery scope (for example the set
/// replayed at daemon boot).
///
/// Scope membership is data filtering only. The scope does not mint observation
/// or execution authority and cannot cause a post-boot action to enter recovery.
pub async fn plan_attached_consequential_recovery_scoped<P: WindowsObserveProvider>(
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    scope: &ConsequentialRecoveryActionScope,
) -> Result<WindowsUiaAttachedRecoveryPlan, WindowsUiaAttachedRecoveryPlanError> {
    plan_attached_consequential_recovery_filtered(journal, runtime, session_id, |action_id| {
        scope.contains(action_id)
    })
    .await
}

async fn plan_attached_consequential_recovery_filtered<P, F>(
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    include_action: F,
) -> Result<WindowsUiaAttachedRecoveryPlan, WindowsUiaAttachedRecoveryPlanError>
where
    P: WindowsObserveProvider,
    F: Fn(Uuid) -> bool,
{
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
        .filter(|binding| include_action(binding.action_id))
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

/// Drain durable consequential recovery debt for the exact currently attached
/// Windows UIA provider/target incarnation.
///
/// This API intentionally has no executor or dispatch-permit parameter. Restart
/// recovery can observe, independently verify, reconcile, or commit an already
/// verified durable receipt, but it can never recreate process-local dispatch
/// authority or blind-retry an action.
pub async fn recover_attached_consequential_debt<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    verifier: &V,
) -> Result<WindowsUiaAttachedRecoveryDrain, WindowsUiaAttachedRecoveryDrainError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let plan = plan_attached_consequential_recovery(journal, runtime, session_id).await?;
    drain_attached_recovery_plan(bridge, journal, runtime, plan, verifier).await
}

/// Drain only consequential actions that were admitted to an immutable recovery
/// epoch. This is the boot-watcher path: later actions in the same journal remain
/// outside the recovery scope even if their session/provider/target lineage is
/// identical to the current attachment.
pub async fn recover_attached_consequential_debt_scoped<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    verifier: &V,
    scope: &ConsequentialRecoveryActionScope,
) -> Result<WindowsUiaAttachedRecoveryDrain, WindowsUiaAttachedRecoveryDrainError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let plan =
        plan_attached_consequential_recovery_scoped(journal, runtime, session_id, scope).await?;
    drain_attached_recovery_plan(bridge, journal, runtime, plan, verifier).await
}

async fn drain_attached_recovery_plan<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    plan: WindowsUiaAttachedRecoveryPlan,
    verifier: &V,
) -> Result<WindowsUiaAttachedRecoveryDrain, WindowsUiaAttachedRecoveryDrainError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let mut outcomes = Vec::with_capacity(plan.entries.len());

    for entry in &plan.entries {
        let outcome = match entry.disposition {
            ConsequentialRecoveryDebtDisposition::NoDispatchProven => {
                WindowsUiaAttachedRecoveryDrainOutcome::NoDispatchProven {
                    action_id: entry.action_id,
                    durable_state: entry.recovery_state,
                }
            }
            ConsequentialRecoveryDebtDisposition::ObservationRequired
            | ConsequentialRecoveryDebtDisposition::CommitOnly => {
                let recovered = recover_consequential_uia_action(
                    bridge,
                    journal,
                    runtime,
                    entry.action_id,
                    verifier,
                )
                .await
                .map_err(|source| WindowsUiaAttachedRecoveryDrainError::Recovery {
                    action_id: entry.action_id,
                    source,
                })?;
                WindowsUiaAttachedRecoveryDrainOutcome::Recovered(recovered)
            }
            ConsequentialRecoveryDebtDisposition::HistoricalTerminal => {
                WindowsUiaAttachedRecoveryDrainOutcome::HistoricalTerminal {
                    action_id: entry.action_id,
                    durable_state: entry.recovery_state,
                }
            }
            ConsequentialRecoveryDebtDisposition::ReconciliationRequired => {
                WindowsUiaAttachedRecoveryDrainOutcome::ReconciliationRequired {
                    action_id: entry.action_id,
                    durable_state: entry.recovery_state,
                }
            }
        };
        outcomes.push(outcome);
    }

    Ok(WindowsUiaAttachedRecoveryDrain {
        session_id: plan.session_id,
        provider_incarnation_ref: plan.provider_incarnation_ref,
        target_incarnation_ref: plan.target_incarnation_ref,
        entries: outcomes,
    })
}
