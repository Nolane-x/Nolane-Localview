use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{CanonicalActionOperation, CanonicalQueuedAction};

use super::{
    ConsequentialJournal, ConsequentialJournalError, ConsequentialJournalTransition,
    ConsequentialRecoveryState, recovery_state_for,
};

const SET_VALUE_COMMITMENT_DOMAIN: &[u8] = b"localview:set-value-payload:v1\0";
pub const SET_VALUE_COMMITMENT_ALGORITHM: &str = "hmac-sha256-process-v1";
const SET_VALUE_COMMITMENT_BYTES: usize = 32;
const SET_VALUE_COMMITMENT_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SetValueMode {
    ReplaceValue,
    ClearValue,
}

impl SetValueMode {
    const fn commitment_tag(self) -> u8 {
        match self {
            Self::ReplaceValue => 0x01,
            Self::ClearValue => 0x02,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SetValuePayloadRef(pub Uuid);

/// Process-local commitment key. The bytes are never serializable or cloneable
/// and are explicitly zeroized when the owner is dropped.
pub struct SetValueCommitmentKey {
    bytes: Zeroizing<[u8; SET_VALUE_COMMITMENT_KEY_BYTES]>,
}

impl SetValueCommitmentKey {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0_u8; SET_VALUE_COMMITMENT_KEY_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self {
            bytes: Zeroizing::new(bytes),
        })
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableSetValuePayloadBinding {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    pub payload_utf8_len: u64,
    pub commitment_algorithm: String,
    pub commitment_digest: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SetValuePayloadVerificationError {
    #[error("SetValue payload reference is invalid")]
    InvalidPayloadRef,
    #[error("SetValue payload commitment algorithm is unsupported")]
    UnsupportedCommitmentAlgorithm,
    #[error("SetValue payload length does not match durable binding")]
    PayloadLengthMismatch,
    #[error("SetValue payload commitment could not be initialized")]
    CommitmentInitializationFailed,
    #[error("SetValue payload commitment does not match durable binding")]
    CommitmentMismatch,
}

pub fn verify_set_value_payload_binding(
    key: &SetValueCommitmentKey,
    binding: &DurableSetValuePayloadBinding,
    payload: &[u8],
) -> Result<(), SetValuePayloadVerificationError> {
    if binding.payload_ref.0.is_nil() {
        return Err(SetValuePayloadVerificationError::InvalidPayloadRef);
    }
    if binding.commitment_algorithm != SET_VALUE_COMMITMENT_ALGORITHM {
        return Err(SetValuePayloadVerificationError::UnsupportedCommitmentAlgorithm);
    }
    if binding.payload_utf8_len != payload.len() as u64 {
        return Err(SetValuePayloadVerificationError::PayloadLengthMismatch);
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| SetValuePayloadVerificationError::CommitmentInitializationFailed)?;
    update_commitment_mac(
        &mut mac,
        binding.action_id,
        binding.payload_ref,
        binding.mode,
        binding.payload_utf8_len,
        payload,
    );
    mac.verify_slice(&binding.commitment_digest)
        .map_err(|_| SetValuePayloadVerificationError::CommitmentMismatch)
}

impl ConsequentialJournal {
    /// Persist payload-free correctness metadata for one exact admitted SetValue
    /// action. The caller plaintext is used only to compute the keyed commitment
    /// and is never written to the companion record.
    pub async fn record_set_value_payload_binding(
        &self,
        queued: &CanonicalQueuedAction,
        key: &SetValueCommitmentKey,
        payload_ref: SetValuePayloadRef,
        mode: SetValueMode,
        payload: &[u8],
    ) -> Result<DurableSetValuePayloadBinding, ConsequentialJournalError> {
        let action_id = queued.action.id;
        if queued.envelope.transport_action_id != action_id
            || queued.envelope.session_id != queued.action.session_id
            || payload_ref.0.is_nil()
        {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "set_value_payload_binding_action_mismatch",
                current: None,
            });
        }

        if self.admitted_operation(action_id).await? != Some(CanonicalActionOperation::SetValue) {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "set_value_payload_binding_without_set_value_operation",
                current: self.recovery_state(action_id).await,
            });
        }

        let state = self.state.lock().await;
        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::Admitted) {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "set_value_payload_binding_after_authorization",
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
                attempted: "set_value_payload_binding_without_exact_intent",
                current,
            })?;

        let payload_utf8_len = u64::try_from(payload.len()).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: format!("SetValue payload length could not be represented: {error}"),
            }
        })?;
        let commitment_digest =
            compute_commitment(key, action_id, payload_ref, mode, payload_utf8_len, payload)?;
        let binding = DurableSetValuePayloadBinding {
            action_id,
            intent_journal_sequence: intent_entry.journal_sequence,
            payload_ref,
            mode,
            payload_utf8_len,
            commitment_algorithm: SET_VALUE_COMMITMENT_ALGORITHM.into(),
            commitment_digest,
        };
        let encoded = serde_json::to_vec(&binding).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: error.to_string(),
            }
        })?;
        let journal_path = Arc::clone(&self.path);
        let binding_path = set_value_payload_binding_path(journal_path.as_path(), action_id);

        // Keep the state lock until the immutable companion is durable. This
        // prevents authorization from being sequenced ahead of the payload
        // commitment fsync in the same journal instance.
        tokio::task::spawn_blocking(move || {
            write_binding_create_new(&binding_path, encoded.as_slice())
        })
        .await
        .map_err(|error| ConsequentialJournalError::Worker {
            message: error.to_string(),
        })??;
        drop(state);

        Ok(binding)
    }

    pub async fn set_value_payload_binding(
        &self,
        action_id: Uuid,
    ) -> Result<Option<DurableSetValuePayloadBinding>, ConsequentialJournalError> {
        if self.admitted_operation(action_id).await? != Some(CanonicalActionOperation::SetValue) {
            return Ok(None);
        }

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

        let binding_path = set_value_payload_binding_path(journal_path.as_path(), action_id);
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
            || binding.payload_ref.0.is_nil()
            || binding.commitment_algorithm != SET_VALUE_COMMITMENT_ALGORITHM
            || binding.commitment_digest.len() != SET_VALUE_COMMITMENT_BYTES
        {
            return Err(ConsequentialJournalError::Serialization {
                message: "SetValue payload binding does not match admitted intent".into(),
            });
        }
        Ok(Some(binding))
    }
}

fn compute_commitment(
    key: &SetValueCommitmentKey,
    action_id: Uuid,
    payload_ref: SetValuePayloadRef,
    mode: SetValueMode,
    payload_utf8_len: u64,
    payload: &[u8],
) -> Result<Vec<u8>, ConsequentialJournalError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).map_err(|_| {
        ConsequentialJournalError::Serialization {
            message: "SetValue payload commitment could not be initialized".into(),
        }
    })?;
    update_commitment_mac(
        &mut mac,
        action_id,
        payload_ref,
        mode,
        payload_utf8_len,
        payload,
    );
    Ok(mac.finalize().into_bytes().to_vec())
}

fn update_commitment_mac(
    mac: &mut Hmac<Sha256>,
    action_id: Uuid,
    payload_ref: SetValuePayloadRef,
    mode: SetValueMode,
    payload_utf8_len: u64,
    payload: &[u8],
) {
    mac.update(SET_VALUE_COMMITMENT_DOMAIN);
    mac.update(action_id.as_bytes());
    mac.update(payload_ref.0.as_bytes());
    mac.update(&[mode.commitment_tag()]);
    mac.update(&payload_utf8_len.to_be_bytes());
    mac.update(payload);
}

fn set_value_payload_binding_path(journal_path: &Path, action_id: Uuid) -> PathBuf {
    PathBuf::from(format!(
        "{}.set-value-payload-{action_id}.json",
        journal_path.display()
    ))
}

fn write_binding_create_new(path: &Path, encoded: &[u8]) -> Result<(), ConsequentialJournalError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "create_set_value_payload_binding",
            message: error.to_string(),
        })?;
    file.write_all(encoded)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "write_set_value_payload_binding",
            message: error.to_string(),
        })?;
    file.flush()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "flush_set_value_payload_binding",
            message: error.to_string(),
        })?;
    file.sync_all()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "sync_set_value_payload_binding",
            message: error.to_string(),
        })?;
    Ok(())
}

fn read_binding(
    path: &Path,
) -> Result<Option<DurableSetValuePayloadBinding>, ConsequentialJournalError> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ConsequentialJournalError::Io {
                operation: "open_set_value_payload_binding",
                message: error.to_string(),
            });
        }
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "read_set_value_payload_binding",
            message: error.to_string(),
        })?;
    serde_json::from_slice(bytes.as_slice())
        .map(Some)
        .map_err(|error| ConsequentialJournalError::Serialization {
            message: format!("invalid SetValue payload binding: {error}"),
        })
}
