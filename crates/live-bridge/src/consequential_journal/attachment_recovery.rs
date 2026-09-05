use std::collections::HashMap;

use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use uuid::Uuid;

use super::{
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState,
    recovery_state_for,
};

/// One durable consequential action whose admitted lineage matches an exact
/// provider attachment after restart.
///
/// This is replay-derived recovery debt only. It does not carry or recreate a
/// dispatch permit, observation grant, executor, or any other process-local
/// authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialAttachmentRecoveryDebt {
    pub action_id: Uuid,
    pub session_id: SessionId,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
}

impl ConsequentialJournal {
    /// Return durable action debt admitted for one exact attached incarnation.
    ///
    /// Matching is exact across runtime session, provider incarnation, and
    /// target incarnation. Ordering uses the latest durable journal sequence;
    /// wall-clock timestamps are deliberately irrelevant to correctness.
    pub async fn recovery_debt_for_attachment(
        &self,
        session_id: SessionId,
        provider_incarnation_ref: &ProviderIncarnationRef,
        target_incarnation_ref: &TargetIncarnationRef,
    ) -> Vec<ConsequentialAttachmentRecoveryDebt> {
        let state = self.state.lock().await;
        let mut latest_sequence_by_action = HashMap::new();
        let mut envelope_by_action = HashMap::new();

        for entry in &state.entries {
            latest_sequence_by_action.insert(entry.action_id, entry.journal_sequence);
            if let ConsequentialJournalTransition::IntentAdmitted { envelope } = &entry.transition {
                envelope_by_action.insert(entry.action_id, envelope.clone());
            }
        }

        let mut debt = envelope_by_action
            .into_iter()
            .filter(|(_, envelope)| {
                envelope.session_id == session_id
                    && envelope.metadata.provider_incarnation_ref == *provider_incarnation_ref
                    && envelope.metadata.target_incarnation_ref == *target_incarnation_ref
            })
            .filter_map(|(action_id, envelope)| {
                let latest_journal_sequence =
                    latest_sequence_by_action.get(&action_id).copied()?;
                let recovery_state = recovery_state_for(&state.entries, action_id)?;
                Some(ConsequentialAttachmentRecoveryDebt {
                    action_id,
                    session_id: envelope.session_id,
                    provider_incarnation_ref: envelope.metadata.provider_incarnation_ref,
                    target_incarnation_ref: envelope.metadata.target_incarnation_ref,
                    expected_postcondition_contract_refs: envelope
                        .metadata
                        .expected_postcondition_contract_refs,
                    recovery_state,
                    latest_journal_sequence,
                })
            })
            .collect::<Vec<_>>();
        debt.sort_by_key(|entry| entry.latest_journal_sequence);
        debt
    }
}
