use anyhow::{bail, Context, Result};
use localview_live_bridge::{ConsequentialJournal, ConsequentialRecoveryState};
use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use localview_windows_observe_runtime::{
    WindowsUiaCommitOnlyRecoveryOutcome, WindowsUiaObserveRuntimeManager,
    recover_consequential_uia_commit_only,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentRecoveryDisposition {
    CommitOnly,
    HistoricalCommitted,
    VerifierRequired,
    NoProviderRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentRecoveryPlanEntry {
    pub action_id: Uuid,
    pub recovery_state: ConsequentialRecoveryState,
    pub disposition: AttachmentRecoveryDisposition,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub latest_journal_sequence: u64,
}

#[derive(Debug, Default)]
pub(crate) struct AttachmentRecoveryReport {
    pub committed_action_ids: Vec<Uuid>,
    pub historical_committed_action_ids: Vec<Uuid>,
    pub verifier_required: Vec<AttachmentRecoveryPlanEntry>,
    pub no_provider_recovery: usize,
}

pub(crate) fn classify_attachment_recovery_state(
    state: ConsequentialRecoveryState,
) -> AttachmentRecoveryDisposition {
    match state {
        ConsequentialRecoveryState::VerifiedUncommitted => {
            AttachmentRecoveryDisposition::CommitOnly
        }
        ConsequentialRecoveryState::Committed => {
            AttachmentRecoveryDisposition::HistoricalCommitted
        }
        ConsequentialRecoveryState::DispatchPrepared
        | ConsequentialRecoveryState::PossiblyDispatched
        | ConsequentialRecoveryState::OutcomeObservedUnverified => {
            AttachmentRecoveryDisposition::VerifierRequired
        }
        ConsequentialRecoveryState::Admitted
        | ConsequentialRecoveryState::AuthorizedNotDispatched
        | ConsequentialRecoveryState::KnownNotDispatched
        | ConsequentialRecoveryState::Compensated
        | ConsequentialRecoveryState::CompensationFailed => {
            AttachmentRecoveryDisposition::NoProviderRecovery
        }
    }
}

pub(crate) async fn plan_attachment_recovery(
    journal: &ConsequentialJournal,
    session_id: SessionId,
    provider_incarnation_ref: &ProviderIncarnationRef,
    target_incarnation_ref: &TargetIncarnationRef,
) -> Vec<AttachmentRecoveryPlanEntry> {
    journal
        .recovery_debt_for_attachment(
            session_id,
            provider_incarnation_ref,
            target_incarnation_ref,
        )
        .await
        .into_iter()
        .map(|debt| AttachmentRecoveryPlanEntry {
            action_id: debt.action_id,
            recovery_state: debt.recovery_state,
            disposition: classify_attachment_recovery_state(debt.recovery_state),
            expected_postcondition_contract_refs: debt.expected_postcondition_contract_refs,
            latest_journal_sequence: debt.latest_journal_sequence,
        })
        .collect()
}

/// Process only recovery work that is safe for the exact currently attached
/// provider/target lineage.
///
/// The runtime snapshot supplies immutable attachment lineage. This function
/// never creates an observation grant and has no verifier or executor argument.
/// Provider-dependent debt is therefore surfaced as `VerifierRequired` rather
/// than being laundered into a world-success claim.
pub(crate) async fn process_windows_attachment_recovery(
    journal: &ConsequentialJournal,
    runtime: &WindowsUiaObserveRuntimeManager,
    session_id: SessionId,
) -> Result<AttachmentRecoveryReport> {
    let snapshot = runtime
        .current_semantic_snapshot(session_id)
        .await
        .context("attached Windows UIA session has no current semantic snapshot")?;
    let plan = plan_attachment_recovery(
        journal,
        session_id,
        snapshot.provider_incarnation_ref(),
        snapshot.target_incarnation_ref(),
    )
    .await;

    process_attachment_recovery_plan(journal, plan).await
}

async fn process_attachment_recovery_plan(
    journal: &ConsequentialJournal,
    plan: Vec<AttachmentRecoveryPlanEntry>,
) -> Result<AttachmentRecoveryReport> {
    let mut report = AttachmentRecoveryReport::default();

    for entry in plan {
        match entry.disposition {
            AttachmentRecoveryDisposition::CommitOnly => {
                match recover_consequential_uia_commit_only(journal, entry.action_id)
                    .await
                    .with_context(|| {
                        format!(
                            "commit durable verified consequential action {}",
                            entry.action_id
                        )
                    })? {
                    WindowsUiaCommitOnlyRecoveryOutcome::CommittedFromDurableReceipt {
                        action_id,
                        ..
                    } => report.committed_action_ids.push(action_id),
                    WindowsUiaCommitOnlyRecoveryOutcome::AlreadyCommitted {
                        action_id,
                        ..
                    } => report.historical_committed_action_ids.push(action_id),
                    WindowsUiaCommitOnlyRecoveryOutcome::NotCommitReady {
                        durable_state,
                        ..
                    } => bail!(
                        "attachment recovery plan for {} became non-commit-ready: {:?}",
                        entry.action_id,
                        durable_state
                    ),
                }
            }
            AttachmentRecoveryDisposition::HistoricalCommitted => {
                match recover_consequential_uia_commit_only(journal, entry.action_id)
                    .await
                    .with_context(|| {
                        format!(
                            "validate durable committed consequential action {}",
                            entry.action_id
                        )
                    })? {
                    WindowsUiaCommitOnlyRecoveryOutcome::AlreadyCommitted {
                        action_id,
                        ..
                    } => report.historical_committed_action_ids.push(action_id),
                    WindowsUiaCommitOnlyRecoveryOutcome::CommittedFromDurableReceipt {
                        action_id,
                        ..
                    } => report.committed_action_ids.push(action_id),
                    WindowsUiaCommitOnlyRecoveryOutcome::NotCommitReady {
                        durable_state,
                        ..
                    } => bail!(
                        "historical attachment recovery state for {} lost durable commit authority: {:?}",
                        entry.action_id,
                        durable_state
                    ),
                }
            }
            AttachmentRecoveryDisposition::VerifierRequired => {
                report.verifier_required.push(entry);
            }
            AttachmentRecoveryDisposition::NoProviderRecovery => {
                report.no_provider_recovery = report.no_provider_recovery.saturating_add(1);
            }
        }
    }

    Ok(report)
}
