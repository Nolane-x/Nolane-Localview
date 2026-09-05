use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::{
    DispatchResult, ProviderIncarnationRef, ReconciliationSnapshotReceipt, SessionId,
    TargetIncarnationRef, TransportResult, WorldOutcome,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::CanonicalActionEnvelope;

const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// Durable evidence that the final authority/freshness fence has been sealed and
/// the action is immediately eligible to cross an external side-effect boundary.
///
/// This record MUST be synced before the provider/executor is allowed to perform
/// the side effect. If the process dies after this record but before a dispatch
/// receipt is committed, recovery treats the action as dispatch-uncertain and
/// requires reconciliation rather than retrying blindly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchPreparationReceipt {
    pub receipt_ref: String,
    pub authorization_journal_sequence: u64,
    pub precondition_snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
}

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
    DispatchPrepared {
        receipt: DispatchPreparationReceipt,
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
    /// A durable write-ahead record exists immediately before the external
    /// side-effect boundary, but no durable executor/dispatch receipt exists.
    /// Recovery must conservatively reconcile and must not blind-retry.
    DispatchPrepared,
    KnownNotDispatched,
    PossiblyDispatched,
    OutcomeObservedUnverified,
    VerifiedUncommitted,
    Compensated,
    CompensationFailed,
    Committed,
}

/// Opaque, process-local authority produced only after the exact PREPARED entry
/// is durable. It is deliberately not Clone/Serialize/Deserialize and its fields
/// are private, so knowing an action id cannot manufacture dispatch authority.
#[derive(Debug, PartialEq, Eq)]
pub struct DispatchPreparedCapability {
    journal_instance_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    capability_ref: Uuid,
}

impl DispatchPreparedCapability {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }
}

/// Durable PREPARED evidence plus the one-shot live capability associated with
/// that exact fsync. Dropping this admission drops the only externally available
/// authority to begin dispatch; durable recovery remains PREPARED/uncertain.
#[derive(Debug, PartialEq, Eq)]
pub struct DispatchPreparedAdmission {
    entry: ConsequentialJournalEntry,
    capability: DispatchPreparedCapability,
}

impl DispatchPreparedAdmission {
    pub fn entry(&self) -> &ConsequentialJournalEntry {
        &self.entry
    }

    pub fn into_parts(self) -> (ConsequentialJournalEntry, DispatchPreparedCapability) {
        (self.entry, self.capability)
    }
}

/// Opaque one-shot execution permit. Creating it consumes the PREPARED
/// capability. Recording the durable dispatch receipt consumes this permit.
#[derive(Debug, PartialEq, Eq)]
pub struct DispatchExecutionPermit {
    journal_instance_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    permit_ref: Uuid,
}

impl DispatchExecutionPermit {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsequentialPostconditionObservationCause {
    DispatchLinearized {
        journal_sequence: u64,
        receipt_ref: String,
    },
    DispatchPreparedUncertain {
        journal_sequence: u64,
        preparation_receipt_ref: String,
    },
}

impl ConsequentialPostconditionObservationCause {
    fn causal_journal_sequence(&self) -> u64 {
        match self {
            Self::DispatchLinearized {
                journal_sequence, ..
            }
            | Self::DispatchPreparedUncertain {
                journal_sequence, ..
            } => *journal_sequence,
        }
    }
}

/// Opaque, process-local authority to perform one post-dispatch observation.
///
/// It is minted only after durable dispatch uncertainty exists and deliberately
/// cannot be cloned or serialized. The fresh snapshot cut is journal-owned so a
/// pre-dispatch snapshot cannot be replayed as postcondition evidence.
#[derive(Debug, PartialEq, Eq)]
pub struct ConsequentialPostconditionObservationPermit {
    journal_instance_ref: Uuid,
    observation_ref: Uuid,
    action_id: Uuid,
    session_id: SessionId,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    snapshot_cut_ref: String,
    cause: ConsequentialPostconditionObservationCause,
}

impl ConsequentialPostconditionObservationPermit {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn cause(&self) -> &ConsequentialPostconditionObservationCause {
        &self.cause
    }
}

/// Runtime-owned proof that an exact reconciliation snapshot was captured using
/// a post-dispatch observation permit. This receipt proves causal observation
/// ordering; it does not by itself prove that any business postcondition passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialPostconditionObservationReceipt {
    action_id: Uuid,
    session_id: SessionId,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    snapshot_cut_ref: String,
    reconciliation_receipt_ref: String,
    cause: ConsequentialPostconditionObservationCause,
}

impl ConsequentialPostconditionObservationReceipt {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn reconciliation_receipt_ref(&self) -> &str {
        &self.reconciliation_receipt_ref
    }

    pub fn cause(&self) -> &ConsequentialPostconditionObservationCause {
        &self.cause
    }

    pub fn causal_journal_sequence(&self) -> u64 {
        self.cause.causal_journal_sequence()
    }
}

#[derive(Debug, Error)]
pub enum ConsequentialPostconditionObservationError {
    #[error(
        "postcondition observation for action {action_id} is invalid from recovery state {current:?}"
    )]
    InvalidRecoveryState {
        action_id: Uuid,
        current: Option<ConsequentialRecoveryState>,
    },
    #[error(
        "dispatch authority is still live for action {action_id}; postcondition observation would race dispatch"
    )]
    DispatchAuthorityStillLive { action_id: Uuid },
    #[error("a postcondition observation is already live for action {action_id}")]
    ObservationAlreadyLive { action_id: Uuid },
    #[error("invalid postcondition observation permit for action {action_id}: {reason}")]
    InvalidPermit {
        action_id: Uuid,
        reason: &'static str,
    },
    #[error("postcondition observation provider incarnation mismatch for action {action_id}")]
    ProviderIncarnationMismatch { action_id: Uuid },
    #[error("postcondition observation target incarnation mismatch for action {action_id}")]
    TargetIncarnationMismatch { action_id: Uuid },
    #[error(
        "postcondition observation cut mismatch for action {action_id}: expected {expected}, got {actual}"
    )]
    SnapshotCutMismatch {
        action_id: Uuid,
        expected: String,
        actual: String,
    },
    #[error(
        "postcondition observation snapshot for action {action_id} has no reconciliation receipt reference"
    )]
    MissingReconciliationReceipt { action_id: Uuid },
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
    #[error("invalid prepared dispatch capability for action {action_id}: {reason}")]
    InvalidDispatchCapability {
        action_id: Uuid,
        reason: &'static str,
    },
    #[error("invalid dispatch execution permit for action {action_id}: {reason}")]
    InvalidDispatchPermit {
        action_id: Uuid,
        reason: &'static str,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct LivePreparedGrant {
    capability_ref: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
}

#[derive(Debug, PartialEq, Eq)]
struct LiveExecutionGrant {
    permit_ref: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePostconditionObservationGrant {
    observation_ref: Uuid,
    session_id: SessionId,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    snapshot_cut_ref: String,
    cause: ConsequentialPostconditionObservationCause,
}

#[derive(Debug, Default)]
struct JournalState {
    next_sequence: u64,
    entries: Vec<ConsequentialJournalEntry>,
    /// Process-local exact capabilities created only after PREPARED fsync.
    /// Reopen intentionally reconstructs none of these grants.
    live_dispatch_prepared: HashMap<Uuid, LivePreparedGrant>,
    /// Process-local permits created only by consuming a matching prepared
    /// capability. They are also never reconstructed from durable state.
    live_dispatch_execution: HashMap<Uuid, LiveExecutionGrant>,
    /// Process-local post-dispatch observation grants. Reopen reconstructs none;
    /// a fresh permit must be minted from durable uncertainty after restart.
    live_postcondition_observation: HashMap<Uuid, LivePostconditionObservationGrant>,
}

#[derive(Clone, Debug)]
pub struct ConsequentialJournal {
    path: Arc<PathBuf>,
    journal_instance_ref: Uuid,
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
            journal_instance_ref: Uuid::new_v4(),
            state: Arc::new(Mutex::new(JournalState {
                next_sequence,
                entries,
                live_dispatch_prepared: HashMap::new(),
                live_dispatch_execution: HashMap::new(),
                live_postcondition_observation: HashMap::new(),
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
                ConsequentialRecoveryState::DispatchPrepared
                    | ConsequentialRecoveryState::PossiblyDispatched
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

    pub async fn record_dispatch_prepared(
        &self,
        action_id: Uuid,
        receipt: DispatchPreparationReceipt,
    ) -> Result<DispatchPreparedAdmission, ConsequentialJournalError> {
        let preparation_receipt_ref = receipt.receipt_ref.clone();
        let mut state = self.state.lock().await;
        let entry = self
            .append_validated_locked(
                &mut state,
                action_id,
                ConsequentialJournalTransition::DispatchPrepared { receipt },
            )
            .await?;

        let capability_ref = Uuid::new_v4();
        let grant = LivePreparedGrant {
            capability_ref,
            preparation_journal_sequence: entry.journal_sequence,
            preparation_receipt_ref: preparation_receipt_ref.clone(),
        };
        state.live_dispatch_prepared.insert(action_id, grant);

        Ok(DispatchPreparedAdmission {
            entry: entry.clone(),
            capability: DispatchPreparedCapability {
                journal_instance_ref: self.journal_instance_ref,
                action_id,
                preparation_journal_sequence: entry.journal_sequence,
                preparation_receipt_ref,
                capability_ref,
            },
        })
    }

    /// Consume the exact live capability associated with one durable PREPARED
    /// record and mint a one-shot execution permit. Durable state intentionally
    /// remains PREPARED until an executor receipt is fsync'd.
    pub async fn begin_dispatch(
        &self,
        capability: DispatchPreparedCapability,
    ) -> Result<DispatchExecutionPermit, ConsequentialJournalError> {
        let action_id = capability.action_id;
        if capability.journal_instance_ref != self.journal_instance_ref {
            return Err(ConsequentialJournalError::InvalidDispatchCapability {
                action_id,
                reason: "journal_instance_mismatch",
            });
        }

        let mut state = self.state.lock().await;
        let Some(grant) = state.live_dispatch_prepared.get(&action_id) else {
            return Err(ConsequentialJournalError::InvalidDispatchCapability {
                action_id,
                reason: "live_prepared_grant_missing",
            });
        };
        if grant.capability_ref != capability.capability_ref
            || grant.preparation_journal_sequence != capability.preparation_journal_sequence
            || grant.preparation_receipt_ref != capability.preparation_receipt_ref
        {
            return Err(ConsequentialJournalError::InvalidDispatchCapability {
                action_id,
                reason: "prepared_grant_binding_mismatch",
            });
        }

        // Consume first. Any failure after this point is deliberately fail-closed:
        // there is no retry capability to restore.
        state.live_dispatch_prepared.remove(&action_id);

        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::DispatchPrepared) {
            return Err(ConsequentialJournalError::InvalidDispatchCapability {
                action_id,
                reason: "durable_state_is_not_prepared",
            });
        }
        if !durable_preparation_matches(
            &state.entries,
            action_id,
            capability.preparation_journal_sequence,
            &capability.preparation_receipt_ref,
        ) {
            return Err(ConsequentialJournalError::InvalidDispatchCapability {
                action_id,
                reason: "durable_prepared_entry_mismatch",
            });
        }

        let permit_ref = Uuid::new_v4();
        state.live_dispatch_execution.insert(
            action_id,
            LiveExecutionGrant {
                permit_ref,
                preparation_journal_sequence: capability.preparation_journal_sequence,
                preparation_receipt_ref: capability.preparation_receipt_ref.clone(),
            },
        );

        Ok(DispatchExecutionPermit {
            journal_instance_ref: self.journal_instance_ref,
            action_id,
            preparation_journal_sequence: capability.preparation_journal_sequence,
            preparation_receipt_ref: capability.preparation_receipt_ref,
            permit_ref,
        })
    }

    /// Commit the executor/dispatch receipt using the exact one-shot permit.
    /// The permit is consumed before serialization/I/O; an error therefore leaves
    /// durable PREPARED state requiring reconciliation and never restores retry
    /// authority.
    pub async fn record_dispatch_linearized(
        &self,
        permit: DispatchExecutionPermit,
        receipt: DispatchLinearizationReceipt,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        let action_id = permit.action_id;
        if permit.journal_instance_ref != self.journal_instance_ref {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "journal_instance_mismatch",
            });
        }

        let mut state = self.state.lock().await;
        let Some(grant) = state.live_dispatch_execution.get(&action_id) else {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "live_execution_grant_missing",
            });
        };
        if grant.permit_ref != permit.permit_ref
            || grant.preparation_journal_sequence != permit.preparation_journal_sequence
            || grant.preparation_receipt_ref != permit.preparation_receipt_ref
        {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "execution_grant_binding_mismatch",
            });
        }

        // Consume first. If durable state changed or append fails, this action is
        // left conservatively PREPARED/uncertain and cannot be blindly retried.
        state.live_dispatch_execution.remove(&action_id);

        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::DispatchPrepared) {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "durable_state_is_not_prepared",
            });
        }
        if !durable_preparation_matches(
            &state.entries,
            action_id,
            permit.preparation_journal_sequence,
            &permit.preparation_receipt_ref,
        ) {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "durable_prepared_entry_mismatch",
            });
        }

        self.append_validated_locked(
            &mut state,
            action_id,
            ConsequentialJournalTransition::DispatchLinearized { receipt },
        )
        .await
    }

    /// Mint one one-shot authority to capture a causally post-dispatch snapshot.
    ///
    /// A live PREPARED capability/execution permit blocks this operation. A
    /// reopened PREPARED journal has no such live grant, so it can reconcile the
    /// crash window without recreating dispatch authority.
    pub async fn begin_postcondition_observation(
        &self,
        action_id: Uuid,
    ) -> Result<
        ConsequentialPostconditionObservationPermit,
        ConsequentialPostconditionObservationError,
    > {
        let mut state = self.state.lock().await;
        let current = recovery_state_for(&state.entries, action_id);

        if state.live_dispatch_prepared.contains_key(&action_id)
            || state.live_dispatch_execution.contains_key(&action_id)
        {
            return Err(
                ConsequentialPostconditionObservationError::DispatchAuthorityStillLive {
                    action_id,
                },
            );
        }
        if state
            .live_postcondition_observation
            .contains_key(&action_id)
        {
            return Err(
                ConsequentialPostconditionObservationError::ObservationAlreadyLive { action_id },
            );
        }
        if !matches!(
            current,
            Some(ConsequentialRecoveryState::DispatchPrepared)
                | Some(ConsequentialRecoveryState::PossiblyDispatched)
                | Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
        ) {
            return Err(
                ConsequentialPostconditionObservationError::InvalidRecoveryState {
                    action_id,
                    current,
                },
            );
        }

        let envelope = admitted_envelope_for(&state.entries, action_id).ok_or(
            ConsequentialPostconditionObservationError::InvalidRecoveryState { action_id, current },
        )?;
        let cause = postcondition_observation_cause_for(&state.entries, action_id).ok_or(
            ConsequentialPostconditionObservationError::InvalidRecoveryState { action_id, current },
        )?;
        let observation_ref = Uuid::new_v4();
        let snapshot_cut_ref = format!(
            "postdispatch:{}:{}:{}:{}",
            self.journal_instance_ref,
            action_id,
            cause.causal_journal_sequence(),
            observation_ref
        );

        let grant = LivePostconditionObservationGrant {
            observation_ref,
            session_id: envelope.session_id,
            provider_incarnation_ref: envelope.metadata.provider_incarnation_ref.clone(),
            target_incarnation_ref: envelope.metadata.target_incarnation_ref.clone(),
            snapshot_cut_ref: snapshot_cut_ref.clone(),
            cause: cause.clone(),
        };
        state
            .live_postcondition_observation
            .insert(action_id, grant);

        Ok(ConsequentialPostconditionObservationPermit {
            journal_instance_ref: self.journal_instance_ref,
            observation_ref,
            action_id,
            session_id: envelope.session_id,
            provider_incarnation_ref: envelope.metadata.provider_incarnation_ref.clone(),
            target_incarnation_ref: envelope.metadata.target_incarnation_ref.clone(),
            snapshot_cut_ref,
            cause,
        })
    }

    /// Consume one exact observation permit and bind the returned provider
    /// snapshot to its fresh cut. The permit is consumed before validating the
    /// snapshot, so a stale/forged completion cannot be retried with the same
    /// authority.
    pub async fn complete_postcondition_observation(
        &self,
        permit: ConsequentialPostconditionObservationPermit,
        snapshot: ReconciliationSnapshotReceipt,
    ) -> Result<
        ConsequentialPostconditionObservationReceipt,
        ConsequentialPostconditionObservationError,
    > {
        let action_id = permit.action_id;
        if permit.journal_instance_ref != self.journal_instance_ref {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "journal_instance_mismatch",
            });
        }

        let mut state = self.state.lock().await;
        let Some(grant) = state
            .live_postcondition_observation
            .get(&action_id)
            .cloned()
        else {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "live_observation_grant_missing",
            });
        };
        if grant.observation_ref != permit.observation_ref
            || grant.session_id != permit.session_id
            || grant.provider_incarnation_ref != permit.provider_incarnation_ref
            || grant.target_incarnation_ref != permit.target_incarnation_ref
            || grant.snapshot_cut_ref != permit.snapshot_cut_ref
            || grant.cause != permit.cause
        {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "observation_grant_binding_mismatch",
            });
        }

        state.live_postcondition_observation.remove(&action_id);
        drop(state);

        if snapshot.provider_incarnation_ref != permit.provider_incarnation_ref {
            return Err(
                ConsequentialPostconditionObservationError::ProviderIncarnationMismatch {
                    action_id,
                },
            );
        }
        if snapshot.target_incarnation_ref != permit.target_incarnation_ref {
            return Err(
                ConsequentialPostconditionObservationError::TargetIncarnationMismatch { action_id },
            );
        }
        if snapshot.snapshot_cut_ref != permit.snapshot_cut_ref {
            return Err(
                ConsequentialPostconditionObservationError::SnapshotCutMismatch {
                    action_id,
                    expected: permit.snapshot_cut_ref,
                    actual: snapshot.snapshot_cut_ref,
                },
            );
        }
        if snapshot.receipt_id.trim().is_empty() {
            return Err(
                ConsequentialPostconditionObservationError::MissingReconciliationReceipt {
                    action_id,
                },
            );
        }

        Ok(ConsequentialPostconditionObservationReceipt {
            action_id,
            session_id: permit.session_id,
            provider_incarnation_ref: permit.provider_incarnation_ref,
            target_incarnation_ref: permit.target_incarnation_ref,
            snapshot_cut_ref: snapshot.snapshot_cut_ref,
            reconciliation_receipt_ref: snapshot.receipt_id,
            cause: permit.cause,
        })
    }

    pub(crate) async fn record_reconciliation_outcome(
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
        self.append_validated_locked(&mut state, action_id, transition)
            .await
    }

    async fn append_validated_locked(
        &self,
        state: &mut JournalState,
        action_id: Uuid,
        transition: ConsequentialJournalTransition,
    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {
        validate_transition(
            &state.entries,
            action_id,
            &transition,
            TransitionValidationMode::Append,
        )?;

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

fn admitted_envelope_for(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
) -> Option<CanonicalActionEnvelope> {
    entries.iter().find_map(|entry| {
        if entry.action_id != action_id {
            return None;
        }
        match &entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope.clone()),
            _ => None,
        }
    })
}

fn postcondition_observation_cause_for(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
) -> Option<ConsequentialPostconditionObservationCause> {
    entries.iter().rev().find_map(|entry| {
        if entry.action_id != action_id {
            return None;
        }
        match &entry.transition {
            ConsequentialJournalTransition::DispatchLinearized { receipt } => Some(
                ConsequentialPostconditionObservationCause::DispatchLinearized {
                    journal_sequence: entry.journal_sequence,
                    receipt_ref: receipt.receipt_ref.clone(),
                },
            ),
            ConsequentialJournalTransition::DispatchPrepared { receipt } => Some(
                ConsequentialPostconditionObservationCause::DispatchPreparedUncertain {
                    journal_sequence: entry.journal_sequence,
                    preparation_receipt_ref: receipt.receipt_ref.clone(),
                },
            ),
            _ => None,
        }
    })
}

fn durable_preparation_matches(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: &str,
) -> bool {
    entries.iter().any(|entry| {
        entry.action_id == action_id
            && entry.journal_sequence == preparation_journal_sequence
            && matches!(
                &entry.transition,
                ConsequentialJournalTransition::DispatchPrepared { receipt }
                    if receipt.receipt_ref == preparation_receipt_ref
            )
    })
}

fn load_and_repair(
    path: &Path,
) -> Result<Vec<ConsequentialJournalEntry>, ConsequentialJournalError> {
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
        .truncate(false)
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
    for (line_offset, newline) in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index))
        .enumerate()
    {
        let line_number = line_offset + 1;
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
        validate_transition(
            &entries,
            entry.action_id,
            &entry.transition,
            TransitionValidationMode::Replay,
        )
        .map_err(|error| ConsequentialJournalError::CorruptRecord {
            line: line_number,
            message: error.to_string(),
        })?;
        entries.push(entry);
        line_start = newline + 1;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionValidationMode {
    Append,
    Replay,
}

fn validate_transition(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
    transition: &ConsequentialJournalTransition,
    mode: TransitionValidationMode,
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
        ConsequentialJournalTransition::DispatchPrepared { receipt } => {
            if current.is_none() {
                return Err(ConsequentialJournalError::UnknownAction { action_id });
            }
            if current != Some(ConsequentialRecoveryState::AuthorizedNotDispatched) {
                return Err(ConsequentialJournalError::InvalidTransition {
                    action_id,
                    attempted: "dispatch_prepared",
                    current,
                });
            }
            validate_dispatch_preparation(entries, action_id, receipt, current)
        }
        ConsequentialJournalTransition::DispatchLinearized { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::DispatchPrepared) => Ok(()),
            // Journals produced before the explicit V4.3 PREPARED boundary are
            // still valid historical evidence. They may be replayed, but new
            // appends cannot use this shortcut.
            Some(ConsequentialRecoveryState::AuthorizedNotDispatched)
                if mode == TransitionValidationMode::Replay =>
            {
                Ok(())
            }
            _ => Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "dispatch_linearized_without_durable_prepare",
                current,
            }),
        },
        ConsequentialJournalTransition::ReconciliationOutcome { .. } => match current {
            None => Err(ConsequentialJournalError::UnknownAction { action_id }),
            Some(ConsequentialRecoveryState::DispatchPrepared)
            | Some(ConsequentialRecoveryState::PossiblyDispatched)
            | Some(ConsequentialRecoveryState::KnownNotDispatched)
            | Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => Ok(()),
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

fn validate_dispatch_preparation(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
    receipt: &DispatchPreparationReceipt,
    current: Option<ConsequentialRecoveryState>,
) -> Result<(), ConsequentialJournalError> {
    let envelope = entries
        .iter()
        .find_map(|entry| {
            if entry.action_id != action_id {
                return None;
            }
            match &entry.transition {
                ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
                _ => None,
            }
        })
        .ok_or(ConsequentialJournalError::UnknownAction { action_id })?;

    let latest_authorization = entries.iter().rev().find_map(|entry| {
        if entry.action_id != action_id {
            return None;
        }
        match &entry.transition {
            ConsequentialJournalTransition::AuthorizationRecorded {
                authorization_revision,
                revalidated,
            } => Some((entry.journal_sequence, authorization_revision, *revalidated)),
            _ => None,
        }
    });
    let Some((authorization_sequence, authorization_revision, revalidated)) = latest_authorization
    else {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_without_authorization",
            current,
        });
    };

    if !revalidated {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_without_revalidated_authority",
            current,
        });
    }
    if authorization_revision != &envelope.metadata.authorization_revision {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_with_stale_authorization_revision",
            current,
        });
    }
    if receipt.receipt_ref.trim().is_empty() {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_without_receipt_ref",
            current,
        });
    }
    if receipt.authorization_journal_sequence != authorization_sequence {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_with_stale_authorization_sequence",
            current,
        });
    }
    if receipt.precondition_snapshot_cut_ref != envelope.metadata.precondition_snapshot_cut_ref {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_with_mismatched_precondition_cut",
            current,
        });
    }
    if receipt.provider_incarnation_ref != envelope.metadata.provider_incarnation_ref {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_with_mismatched_provider_incarnation",
            current,
        });
    }
    if receipt.target_incarnation_ref != envelope.metadata.target_incarnation_ref {
        return Err(ConsequentialJournalError::InvalidTransition {
            action_id,
            attempted: "dispatch_prepared_with_mismatched_target_incarnation",
            current,
        });
    }

    Ok(())
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
            ConsequentialJournalTransition::DispatchPrepared { .. } => {
                ConsequentialRecoveryState::DispatchPrepared
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
