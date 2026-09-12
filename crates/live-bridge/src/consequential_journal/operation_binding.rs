use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{CanonicalActionOperation, CanonicalQueuedAction};

use super::{
    ConsequentialJournal, ConsequentialJournalError, ConsequentialJournalTransition,
    ConsequentialRecoveryState, recovery_state_for,
};

/// Durable, payload-free binding between one admitted canonical intent and the
/// exact operation class that the transport action represented at admission.
///
/// Raw text, keys, pointer coordinates and other action payload never enter this
/// record. The intent journal sequence prevents a stale companion file from
/// being treated as authority for another admission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableCanonicalActionOperationBinding {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub operation: CanonicalActionOperation,
}

impl ConsequentialJournal {
    /// Persist the canonical operation derived from the legacy bridge action.
    /// New semantic product surfaces whose intent is more specific than the
    /// compatibility carrier must use `record_intent_operation_bound_explicit`.
    pub async fn record_intent_operation_bound(
        &self,
        queued: &CanonicalQueuedAction,
    ) -> Result<DurableCanonicalActionOperationBinding, ConsequentialJournalError> {
        let action_id = queued.action.id;
        let operation = CanonicalActionOperation::from_bridge_action_kind(&queued.action.action)
            .ok_or(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "intent_operation_binding_for_internal_action",
                current: None,
            })?;
        self.record_intent_operation_bound_explicit(queued, operation)
            .await
    }

    /// Persist an exact server-owned semantic operation while the canonical
    /// intent is still merely admitted.
    ///
    /// This seam deliberately accepts a typed operation rather than deriving it
    /// from the legacy bridge carrier. It lets product planning bind semantics
    /// such as `Select` without teaching the compatibility wire format a new
    /// public action kind, while keeping the durable authority record explicit.
    pub async fn record_intent_operation_bound_explicit(
        &self,
        queued: &CanonicalQueuedAction,
        operation: CanonicalActionOperation,
    ) -> Result<DurableCanonicalActionOperationBinding, ConsequentialJournalError> {
        let action_id = queued.action.id;

        if queued.envelope.transport_action_id != action_id
            || queued.envelope.session_id != queued.action.session_id
        {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "intent_operation_binding_action_mismatch",
                current: None,
            });
        }

        let state = self.state.lock().await;
        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::Admitted) {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "intent_operation_binding_after_authorization",
                current,
            });
        }

        let intent_entry = state
            .entries
            .iter()
            .find(|entry| {
                entry.action_id == action_id
                    && matches!(
                        &entry.transition,
                        ConsequentialJournalTransition::IntentAdmitted { envelope }
                            if envelope == &queued.envelope
                    )
            })
            .ok_or(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "intent_operation_binding_without_exact_intent",
                current,
            })?;

        let binding = DurableCanonicalActionOperationBinding {
            action_id,
            intent_journal_sequence: intent_entry.journal_sequence,
            operation,
        };
        let encoded = serde_json::to_vec(&binding).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: error.to_string(),
            }
        })?;
        let journal_path = Arc::clone(&self.path);
        let binding_path = operation_binding_path(journal_path.as_path(), action_id);

        // Keep the journal state lock until the companion record is durable.
        // `record_authorization` uses the same lock, so successful revalidation
        // can never be sequenced ahead of this fsync in the same journal instance.
        tokio::task::spawn_blocking(move || write_binding_create_new(&binding_path, &encoded))
            .await
            .map_err(|error| ConsequentialJournalError::Worker {
                message: error.to_string(),
            })??;
        drop(state);

        Ok(binding)
    }

    /// Read and validate the immutable operation companion for one exact
    /// admitted intent. Missing is represented explicitly so dispatch authority
    /// can fail closed without treating historical journals as corrupt.
    pub async fn admitted_operation(
        &self,
        action_id: Uuid,
    ) -> Result<Option<CanonicalActionOperation>, ConsequentialJournalError> {
        let (intent_journal_sequence, journal_path) = {
            let state = self.state.lock().await;
            let Some(intent_entry) = state.entries.iter().find(|entry| {
                entry.action_id == action_id
                    && matches!(
                        entry.transition,
                        ConsequentialJournalTransition::IntentAdmitted { .. }
                    )
            }) else {
                return Ok(None);
            };
            (intent_entry.journal_sequence, Arc::clone(&self.path))
        };

        let binding_path = operation_binding_path(journal_path.as_path(), action_id);
        let binding = tokio::task::spawn_blocking(move || read_binding(&binding_path))
            .await
            .map_err(|error| ConsequentialJournalError::Worker {
                message: error.to_string(),
            })??;

        let Some(binding) = binding else {
            return Ok(None);
        };
        if binding.action_id != action_id
            || binding.intent_journal_sequence != intent_journal_sequence
        {
            return Err(ConsequentialJournalError::Serialization {
                message: "canonical operation binding does not match admitted intent".into(),
            });
        }
        Ok(Some(binding.operation))
    }
}

fn operation_binding_path(journal_path: &Path, action_id: Uuid) -> PathBuf {
    PathBuf::from(format!(
        "{}.operation-{action_id}.json",
        journal_path.display()
    ))
}

fn write_binding_create_new(path: &Path, encoded: &[u8]) -> Result<(), ConsequentialJournalError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "create_operation_binding",
            message: error.to_string(),
        })?;
    file.write_all(encoded)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "write_operation_binding",
            message: error.to_string(),
        })?;
    file.flush()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "flush_operation_binding",
            message: error.to_string(),
        })?;
    file.sync_all()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "sync_operation_binding",
            message: error.to_string(),
        })?;
    Ok(())
}

fn read_binding(
    path: &Path,
) -> Result<Option<DurableCanonicalActionOperationBinding>, ConsequentialJournalError> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ConsequentialJournalError::Io {
                operation: "open_operation_binding",
                message: error.to_string(),
            });
        }
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "read_operation_binding",
            message: error.to_string(),
        })?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        ConsequentialJournalError::Serialization {
            message: format!("invalid canonical operation binding: {error}"),
        }
    })
}
