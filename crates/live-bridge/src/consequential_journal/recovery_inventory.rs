use std::collections::{BTreeSet, HashMap};

use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState,
    recovery_state_for,
};

/// Typed recovery work allowed by a durable consequential state.
///
/// This is classification only. It never recreates a dispatch permit, execution
/// grant, observation grant, verifier, or provider capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsequentialRecoveryDebtDisposition {
    NoDispatchProven,
    ObservationRequired,
    CommitOnly,
    HistoricalTerminal,
    ReconciliationRequired,
}

impl ConsequentialRecoveryState {
    pub fn recovery_debt_disposition(&self) -> ConsequentialRecoveryDebtDisposition {
        use ConsequentialRecoveryDebtDisposition::{
            CommitOnly, HistoricalTerminal, NoDispatchProven, ObservationRequired,
            ReconciliationRequired,
        };

        match self {
            Self::Admitted | Self::AuthorizedNotDispatched | Self::KnownNotDispatched => {
                NoDispatchProven
            }
            // PREPARED remains uncertain after restart. Recovery observes current
            // state and never recreates dispatch authority or retries the action.
            Self::DispatchPrepared | Self::PossiblyDispatched | Self::OutcomeObservedUnverified => {
                ObservationRequired
            }
            Self::VerifiedUncommitted => CommitOnly,
            Self::Compensated | Self::Committed => HistoricalTerminal,
            Self::CompensationFailed => ReconciliationRequired,
        }
    }
}

/// One replay-derived recovery summary per durable action. Ordering authority is
/// the monotonic journal sequence, never wall-clock timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsequentialRecoveryInventoryEntry {
    pub action_id: Uuid,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
}

/// Immutable membership boundary for one recovery epoch such as daemon boot.
///
/// The scope freezes action identity, not journal sequence. Recovery may append
/// newer observation/reconciliation records for an in-scope action and that same
/// action must remain retryable after a transient failure. Actions admitted after
/// the scope was frozen receive a different action id and are therefore excluded.
/// This is data scoping only: it carries no dispatch, execution, observation, or
/// provider authority.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsequentialRecoveryActionScope {
    action_ids: BTreeSet<Uuid>,
}

impl ConsequentialRecoveryActionScope {
    pub fn from_inventory(entries: &[ConsequentialRecoveryInventoryEntry]) -> Self {
        Self {
            action_ids: entries.iter().map(|entry| entry.action_id).collect(),
        }
    }

    pub fn contains(&self, action_id: Uuid) -> bool {
        self.action_ids.contains(&action_id)
    }

    pub fn is_empty(&self) -> bool {
        self.action_ids.is_empty()
    }

    pub fn len(&self) -> usize {
        self.action_ids.len()
    }
}

/// Replay-derived recovery debt bound to the exact durable action lineage.
///
/// This is data authority only. Reconstructing this record never recreates a
/// dispatch permit, execution grant, observation grant, or provider capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialRecoveryBindingEntry {
    pub action_id: Uuid,
    pub session_id: SessionId,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub expected_postcondition_contract_refs: Vec<String>,
    pub recovery_state: ConsequentialRecoveryState,
    pub latest_journal_sequence: u64,
}

impl ConsequentialJournal {
    /// Replay-derived inventory with exactly one entry per durable action.
    ///
    /// The latest journal sequence is the causal ordering key. `recorded_at` is
    /// intentionally ignored so clock skew cannot reorder recovery work.
    pub async fn recovery_inventory(&self) -> Vec<ConsequentialRecoveryInventoryEntry> {
        let state = self.state.lock().await;
        let mut latest_sequence_by_action = HashMap::new();
        for entry in &state.entries {
            latest_sequence_by_action.insert(entry.action_id, entry.journal_sequence);
        }

        let mut inventory = latest_sequence_by_action
            .into_iter()
            .map(|(action_id, latest_journal_sequence)| {
                let recovery_state = recovery_state_for(&state.entries, action_id)
                    .expect("durable action history must have a recovery state");
                ConsequentialRecoveryInventoryEntry {
                    action_id,
                    recovery_state,
                    latest_journal_sequence,
                }
            })
            .collect::<Vec<_>>();
        inventory.sort_by_key(|entry| entry.latest_journal_sequence);
        inventory
    }

    /// Recovery inventory carrying the exact admitted action lineage required to
    /// bind durable debt to a re-attached provider/target incarnation.
    pub async fn recovery_binding_inventory(&self) -> Vec<ConsequentialRecoveryBindingEntry> {
        let state = self.state.lock().await;
        let mut latest_sequence_by_action = HashMap::new();
        let mut admitted_by_action = HashMap::new();
        for entry in &state.entries {
            latest_sequence_by_action.insert(entry.action_id, entry.journal_sequence);
            if let ConsequentialJournalTransition::IntentAdmitted { envelope } = &entry.transition {
                admitted_by_action.insert(entry.action_id, envelope.clone());
            }
        }

        let mut bindings = latest_sequence_by_action
            .into_iter()
            .map(|(action_id, latest_journal_sequence)| {
                let envelope = admitted_by_action
                    .get(&action_id)
                    .expect("durable action history must retain its admitted envelope");
                let recovery_state = recovery_state_for(&state.entries, action_id)
                    .expect("durable action history must have a recovery state");
                ConsequentialRecoveryBindingEntry {
                    action_id,
                    session_id: envelope.session_id,
                    provider_incarnation_ref: envelope.metadata.provider_incarnation_ref.clone(),
                    target_incarnation_ref: envelope.metadata.target_incarnation_ref.clone(),
                    expected_postcondition_contract_refs: envelope
                        .metadata
                        .expected_postcondition_contract_refs
                        .clone(),
                    recovery_state,
                    latest_journal_sequence,
                }
            })
            .collect::<Vec<_>>();
        bindings.sort_by_key(|entry| entry.latest_journal_sequence);
        bindings
    }

    /// Return only recovery debt whose durable session/provider/target lineage is
    /// exactly equal to the currently attached native provider lineage.
    pub async fn recovery_bindings_for_attachment(
        &self,
        session_id: SessionId,
        provider_incarnation_ref: &ProviderIncarnationRef,
        target_incarnation_ref: &TargetIncarnationRef,
    ) -> Vec<ConsequentialRecoveryBindingEntry> {
        self.recovery_binding_inventory()
            .await
            .into_iter()
            .filter(|entry| {
                entry.session_id == session_id
                    && &entry.provider_incarnation_ref == provider_incarnation_ref
                    && &entry.target_incarnation_ref == target_incarnation_ref
            })
            .collect()
    }
}
