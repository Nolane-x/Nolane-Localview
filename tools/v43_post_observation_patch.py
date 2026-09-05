from pathlib import Path

path = Path("crates/live-bridge/src/consequential_journal.rs")
text = path.read_text()

def replace_once(old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected exactly one sentinel, found {count}: {old[:120]!r}")
    text = text.replace(old, new, 1)

replace_once(
'''use localview_protocol::{
    DispatchResult, ProviderIncarnationRef, TargetIncarnationRef, TransportResult, WorldOutcome,
};''',
'''use localview_protocol::{
    DispatchResult, ProviderIncarnationRef, ReconciliationSnapshotReceipt, SessionId,
    TargetIncarnationRef, TransportResult, WorldOutcome,
};''',
)

marker = '''#[derive(Debug, Error)]
pub enum ConsequentialJournalError {'''
insert = r'''#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[error("postcondition observation for action {action_id} is invalid from recovery state {current:?}")]
    InvalidRecoveryState {
        action_id: Uuid,
        current: Option<ConsequentialRecoveryState>,
    },
    #[error("dispatch authority is still live for action {action_id}; postcondition observation would race dispatch")]
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
    #[error("postcondition observation cut mismatch for action {action_id}: expected {expected}, got {actual}")]
    SnapshotCutMismatch {
        action_id: Uuid,
        expected: String,
        actual: String,
    },
    #[error("postcondition observation snapshot for action {action_id} has no reconciliation receipt reference")]
    MissingReconciliationReceipt { action_id: Uuid },
}

#[derive(Debug, Error)]
pub enum ConsequentialJournalError {'''
replace_once(marker, insert)

replace_once(
'''#[derive(Debug, PartialEq, Eq)]
struct LiveExecutionGrant {
    permit_ref: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
}

#[derive(Debug, Default)]
struct JournalState {''',
'''#[derive(Debug, PartialEq, Eq)]
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
struct JournalState {''',
)

replace_once(
'''    /// Process-local permits created only by consuming a matching prepared
    /// capability. They are also never reconstructed from durable state.
    live_dispatch_execution: HashMap<Uuid, LiveExecutionGrant>,
}''',
'''    /// Process-local permits created only by consuming a matching prepared
    /// capability. They are also never reconstructed from durable state.
    live_dispatch_execution: HashMap<Uuid, LiveExecutionGrant>,
    /// Process-local post-dispatch observation grants. Reopen reconstructs none;
    /// a fresh permit must be minted from durable uncertainty after restart.
    live_postcondition_observation: HashMap<Uuid, LivePostconditionObservationGrant>,
}''',
)

replace_once(
'''                live_dispatch_prepared: HashMap::new(),
                live_dispatch_execution: HashMap::new(),
            })),''',
'''                live_dispatch_prepared: HashMap::new(),
                live_dispatch_execution: HashMap::new(),
                live_postcondition_observation: HashMap::new(),
            })),''',
)

method_marker = '''    pub(crate) async fn record_reconciliation_outcome(
'''
methods = r'''    /// Mint one one-shot authority to capture a causally post-dispatch snapshot.
    ///
    /// A live PREPARED capability/execution permit blocks this operation. A
    /// reopened PREPARED journal has no such live grant, so it can reconcile the
    /// crash window without recreating dispatch authority.
    pub async fn begin_postcondition_observation(
        &self,
        action_id: Uuid,
    ) -> Result<ConsequentialPostconditionObservationPermit, ConsequentialPostconditionObservationError>
    {
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
        if state.live_postcondition_observation.contains_key(&action_id) {
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
            ConsequentialPostconditionObservationError::InvalidRecoveryState {
                action_id,
                current,
            },
        )?;
        let cause = postcondition_observation_cause_for(&state.entries, action_id).ok_or(
            ConsequentialPostconditionObservationError::InvalidRecoveryState {
                action_id,
                current,
            },
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
    ) -> Result<ConsequentialPostconditionObservationReceipt, ConsequentialPostconditionObservationError>
    {
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
            return Err(ConsequentialPostconditionObservationError::SnapshotCutMismatch {
                action_id,
                expected: permit.snapshot_cut_ref,
                actual: snapshot.snapshot_cut_ref,
            });
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
'''
replace_once(method_marker, methods)

helper_marker = '''fn durable_preparation_matches(
'''
helpers = r'''fn admitted_envelope_for(
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
            ConsequentialJournalTransition::DispatchLinearized { receipt } => {
                Some(ConsequentialPostconditionObservationCause::DispatchLinearized {
                    journal_sequence: entry.journal_sequence,
                    receipt_ref: receipt.receipt_ref.clone(),
                })
            }
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
'''
replace_once(helper_marker, helpers)

path.write_text(text)
