use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::{DispatchResult, TransportResult, WorldOutcome};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::CanonicalActionEnvelope;

const JOURNAL_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchLinearizationReceipt {
    pub receipt_ref: String,
    pub transport_result: TransportResult,
    pub dispatch_result: DispatchResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsequentialJournalTransition {
    IntentAdmitted {
        envelope: CanonicalActionEnvelope,
    },
    AuthorizationRecorded {
        authorization_revision: String,
        revalidated: bool,
    },
    DispatchLinearized {
        receipt: DispatchLinearizationReceipt,
    },
    ReconciliationOutcome {
        world_outcome: WorldOutcome,
        reconciliation_receipt_ref: Option<String>,
        postcondition_receipt_refs: Vec<String>,
        postconditions_verified: bool,
    },
    CompensationRecorded {
        compensation_ref: String,
        world_outcome: WorldOutcome,
    },
    Committed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsequentialJournalEntry {
    pub schema_version: u32,
    pub journal_sequence: u64,
    pub action_id: Uuid,
    pub recorded_at: DateTime<Utc>,
    pub transition: ConsequentialJournalTransition,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsequentialRecoveryState {
    Admitted,
    AuthorizedNotDispatched,
    KnownNotDispatched,
    PossiblyDispatched,
    OutcomeObservedUnverified,
    VerifiedUncommitted,
    Compensated,
    CompensationFailed,
    Committed,
}

#[derive(Debug, Error)]
pub enum ConsequentialJournalError {
    #[error("journal I/O failed during {operation}: {message}")]
    Io {
        operation: &'static str,
        message: String,
    },
    #[error("journal serialization failed: {message}")]
    Serialization { message: String },
    #[error("journal worker failed: {message}")]
    Worker { message: String },
    #[error("corrupt journal record at line {line}: {message}")]
    CorruptRecord { line: usize, message: String },
    #[error("unknown consequential action {action_id}")]
    UnknownAction { action_id: Uuid },
    #[error("consequential action {action_id} was admitted more than once")]
    DuplicateIntent { action_id: Uuid },
    #[error("invalid journal transition '{attempted}' for action {action_id} from {current:?}")]
    InvalidTransition {
        action_id: Uuid,
        attempted: &'static str,
        current: Option<ConsequentialRecoveryState>,
    },
}

#[derive(Debug, Default)]
struct JournalState {
    next_sequence: u64,
    entries: Vec<ConsequentialJournalEntry>,
}

#[derive(Clone, Debug)]
pub struct ConsequentialJournal {
    path: Arc<PathBuf>,
    state: Arc<Mutex<JournalState>>,
}

impl ConsequentialJournal {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, ConsequentialJournalError> {
        let path = path.as_ref().to_path_buf();
        let load_path = path.clone();
        let entries = tokio::task::spawn_blocking(move || load_and_repair(&load_path))
            .await
            .map_err(|error| ConsequentialJournalError::Worker {
                message: error.to_string(),
            })??;
        let next_sequence = entries
            .last()
            .map(|entry| entry.journal_sequence.saturating_add(1))
            .unwrap_or(1);

        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(JournalState {
                next_sequence,
                entries,
            })),
        })
    }

    pub async fn entries_for(&self, action_id: Uuid) -> Vec<ConsequentialJournalEntry> {
        self.state
            .lock()
            .await
            .entries
            .iter()
            .filter(|entry| entry.action_id == action_id)
            .cloned()
            .collect()
    }

    pub async fn recovery_state(&self, action_id: Uuid) -> Option<ConsequentialRecoveryState> {
        let state = self.state.lock().await;
        recovery_state_for(&state.entries, action_id)
    }

    pub async fn requires_reconciliation(&self, action_id: Uuid) -> Option<bool> {
        self.recovery_state(action_id).await.map(|state| {
            matches!(
                state,
                ConsequentialRecoveryState::PossiblyDispatched
                    | ConsequentialRecoveryState::OutcomeObservedUnverified
                    | ConsequentialRecoveryState::CompensationFailed
            )
        })
    }

    pub async fn record_intent_admitted(
        &self,
        envelope: CanonicalActionEnvelope,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        let action_id = envelope.transport_action_id;
        self.append_validated(
            action_id,
            ConsequentialJournalTransition::IntentAdmitted { envelope },
        )
        .await
    }

    pub async fn record_authorization(
        &self,
        action_id: Uuid,
        authorization_revision: String,
        revalidated: bool,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        self.append_validated(
            action_id,
            ConsequentialJournalTransition::AuthorizationRecorded {
                authorization_revision,
                revalidated,
            },
        )
        .await
    }

    pub async fn record_dispatch_linearized(
        &self,
        action_id: Uuid,
        receipt: DispatchLinearizationReceipt,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        self.append_validated(
            action_id,
            ConsequentialJournalTransition::DispatchLinearized { receipt },
        )
        .await
    }

    pub async fn record_reconciliation_outcome(
        &self,
        action_id: Uuid,
        world_outcome: WorldOutcome,
        reconciliation_receipt_ref: Option<String>,
        postcondition_receipt_refs: Vec<String>,
        postconditions_verified: bool,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        self.append_validated(
            action_id,
            ConsequentialJournalTransition::ReconciliationOutcome {
                world_outcome,
                reconciliation_receipt_ref,
                postcondition_receipt_refs,
                postconditions_verified,
            },
        )
        .await
    }

    pub async fn record_compensation(
        &self,
        action_id: Uuid,
        compensation_ref: String,
        world_outcome: WorldOutcome,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        self.append_validated(
            action_id,
            ConsequentialJournalTransition::CompensationRecorded {
                compensation_ref,
                world_outcome,
            },
        )
        .await
    }

    pub async fn record_committed(
        &self,
        action_id: Uuid,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        self.append_validated(action_id, ConsequentialJournalTransition::Committed)
            .await
    }

    async fn append_validated(
        &self,
        action_id: Uuid,
        transition: ConsequentialJournalTransition,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        let mut state = self.state.lock().await;
        validate_transition(&state.entries, action_id, &transition)?;

        let entry = ConsequentialJournalEntry {
            schema_version: JOURNAL_SCHEMA_VERSION,
            journal_sequence: state.next_sequence,
            action_id,
            recorded_at: Utc::now(),
            transition,
        };
        let mut encoded = serde_json::to_vec(&entry).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: error.to_string(),
            }
        })?;
        encoded.push(b'\n');

        let path = Arc::clone(&self.path);
        tokio::task::spawn_blocking(move || append_durable(path.as_path(), &encoded))
            .await
            .map_err(|error| ConsequentialJournalError::Worker {
                message: error.to_string(),
            })??;

        state.next_sequence = state.next_sequence.saturating_add(1);
        state.entries.push(entry.clone());
        Ok(entry)
    }
}

fn load_and_repair(path: &Path) -> Result<Vec<ConsequentialJournalEntry>, ConsequentialJournalError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| ConsequentialJournalError::Io {
                operation: "create_parent",
                message: error.to_string(),
            })?;
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "open",
            message: error.to_string(),
        })?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "read",
            message: error.to_string(),
        })?;

    let durable_len = if bytes.is_empty() || bytes.ends_with(b"\n") {
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(0)
    };

    if durable_len != bytes.len() {
        file.set_len(durable_len as u64)
            .map_err(|error| ConsequentialJournalError::Io {
                operation: "truncate_incomplete_tail",
                message: error.to_string(),
            })?;
        file.sync_all()
            .map_err(|error| ConsequentialJournalError::Io {
                operation: "sync_truncated_tail",
                message: error.to_string(),
            })?;
        bytes.truncate(durable_len);
    } else if bytes.is_empty() {
        file.sync_all()
            .map_err(|error| ConsequentialJournalError::Io {
                operation: "sync_created_journal",
                message: error.to_string(),
            })?;
    }

    let mut entries = Vec::new();
    let mut line_start = 0usize;
    let mut line_number = 1usize;
    for newline in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index))
    {
        let line = &bytes[line_start..newline];
        if line.is_empty() {
            return Err(ConsequentialJournalError::CorruptRecord {
                line: line_number,
                message: "empty durable record".into(),
            });
        }
        let entry: ConsequentialJournalEntry = serde_json::from_slice(line).map_err(|error| {
            ConsequentialJournalError::CorruptRecord {
                line: line_number,
                message: error.to_string(),
            }
        })?;
        let expected_sequence = entries.len() as u64 + 1;
        if entry.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(ConsequentialJournalError::CorruptRecord {
                line: line_number,
                message: format!(
                    "unsupported schema version {}; expected {}",
                    entry.schema_version, JOURNAL_SCHEMA_VERSION
                ),
            });
        }
        if entry.journal_sequence != expected_sequence {
            return Err(ConsequentialJournalError::CorruptRecord {
                line: line_number,
                message: format!(
                    "journal sequence {}; expected {}",
                    entry.journal_sequence, expected_sequence
                ),
            });
        }
        validate_transition(&entries, entry.action_id, &entry.transition).map_err(|error| {
            ConsequentialJournalError::CorruptRecord {
                line: line_number,
                message: error.to_string(),
            }
        })?;
        entries.push(entry);
        line_start = newline + 1;
        line_number += 1;
    }

    Ok(entries)
}

fn append_durable(path: &Path, encoded: &[u8]) -> Result<(), ConsequentialJournalError> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "append_open",
            message: error.to_string(),
        })?;
    file.write_all(encoded)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "append_write",
            message: error.to_string(),
        })?;
    file.flush()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "append_flush",
            message: error.to_string(),
        })?;
    file.sync_all()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "append_sync",
            message: error.to_string(),
        })?;
    Ok(())
}

fn validate_transition(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
    transition: &ConsequentialJournalTransition,
) -> Result<(), ConsequentialJournalError> {
    let current = recovery_state_for(entries, action_id);
    match transition {
        ConsequentialJournalTransition::IntentAdmitted { envelope } => {
            if current.is_some() {
                return Err(ConsequentialJournalError::DuplicateIntent { action_id });
            }
            if envelope.transport_action_id != action_id {
                return Err(ConsequentialJournalError::InvalidTransition {
                    action_id,
                    attempted: "intent_admitted_with_mismatched_transport_action",
                    current,
                });
            }
            Ok(())
        }
        ConsequentialJournalTransition::AuthorizationRecorded { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::Admitted)
            | Some(ConsequentialRecoveryState::AuthorizedNotDispatched) => Ok(()),
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "authorization_recorded",
                current,
            }),
        },
        ConsequentialJournalTransition::DispatchLinearized { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::AuthorizedNotDispatched) => Ok(()),
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "dispatch_linearized",
                current,
            }),
        },
        ConsequentialJournalTransition::ReconciliationOutcome { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::PossiblyDispatched)
            | Some(ConsequentialRecoveryState::KnownNotDispatched) => Ok(()),
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "reconciliation_outcome",
                current,
            }),
        },
        ConsequentialJournalTransition::CompensationRecorded { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => Ok(()),
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "compensation_recorded",
                current,
            }),
        },
        ConsequentialJournalTransition::Committed => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::VerifiedUncommitted) => Ok(()),
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "committed",
                current,
            }),
        },
    }
}

fn recovery_state_for(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
) -> Option<ConsequentialRecoveryState> {
    let mut state = None;
    for entry in entries.iter().filter(|entry| entry.action_id == action_id) {
        state = Some(match &entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { .. } => {
                ConsequentialRecoveryState::Admitted
            }
            ConsequentialJournalTransition::AuthorizationRecorded { .. } => {
                ConsequentialRecoveryState::AuthorizedNotDispatched
            }
            ConsequentialJournalTransition::DispatchLinearized { receipt } => {
                match receipt.dispatch_result {
                    DispatchResult::DispatchedFull
                    | DispatchResult::DispatchedPartial
                    | DispatchResult::DispatchAmbiguous => {
                        ConsequentialRecoveryState::PossiblyDispatched
                    }
                    DispatchResult::NotDispatched
                    | DispatchResult::DispatchRejected
                    | DispatchResult::DispatchBlockedPermission
                    | DispatchResult::DispatchBlockedIdentity
                    | DispatchResult::DispatchBlockedFocus
                    | DispatchResult::DispatchBlockedProvider => {
                        ConsequentialRecoveryState::KnownNotDispatched
                    }
                }
            }
            ConsequentialJournalTransition::ReconciliationOutcome {
                world_outcome,
                postconditions_verified,
                ..
            } => {
                if *postconditions_verified && *world_outcome == WorldOutcome::VerifiedExpected {
                    ConsequentialRecoveryState::VerifiedUncommitted
                } else {
                    ConsequentialRecoveryState::OutcomeObservedUnverified
                }
            }
            ConsequentialJournalTransition::CompensationRecorded { world_outcome, .. } => {
                if *world_outcome == WorldOutcome::CompensatedVerified {
                    ConsequentialRecoveryState::Compensated
                } else {
                    ConsequentialRecoveryState::CompensationFailed
                }
            }
            ConsequentialJournalTransition::Committed => ConsequentialRecoveryState::Committed,
        });
    }
    state
}
