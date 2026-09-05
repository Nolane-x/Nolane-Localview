use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{recovery_state_for, ConsequentialJournal, ConsequentialRecoveryState};

/// One replay-derived recovery summary per durable action. Ordering authority is
/// the monotonic journal sequence, never wall-clock timestamps.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsequentialRecoveryInventoryEntry {
    pub action_id: Uuid,
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
}
