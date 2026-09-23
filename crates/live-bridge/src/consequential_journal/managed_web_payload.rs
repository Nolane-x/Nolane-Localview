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

const MANAGED_WEB_PAYLOAD_COMMITMENT_DOMAIN: &[u8] =
    b"localview:managed-web-payload:v1\0";
pub const MANAGED_WEB_PAYLOAD_COMMITMENT_ALGORITHM: &str =
    "hmac-sha256-process-v1";
const MANAGED_WEB_PAYLOAD_COMMITMENT_BYTES: usize = 32;
const MANAGED_WEB_PAYLOAD_COMMITMENT_KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ManagedWebPayloadRef(pub Uuid);

/// Process-local key used to commit payload-bearing managed-WebView actions.
///
/// The key is intentionally non-serializable and non-cloneable. Restart therefore
/// preserves durable debt/outcome metadata without restoring payload authority.
pub struct ManagedWebPayloadCommitmentKey {
    bytes: Zeroizing<[u8; MANAGED_WEB_PAYLOAD_COMMITMENT_KEY_BYTES]>,
}

impl ManagedWebPayloadCommitmentKey {
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0_u8; MANAGED_WEB_PAYLOAD_COMMITMENT_KEY_BYTES];
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
pub struct DurableManagedWebPayloadBinding {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub payload_ref: ManagedWebPayloadRef,
    pub operation: CanonicalActionOperation,
    pub payload_len: u64,
    pub commitment_algorithm: String,
    pub commitment_digest: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ManagedWebPayloadVerificationError {
    #[error("managed-WebView payload reference is invalid")]
    InvalidPayloadRef,
    #[error("managed-WebView payload operation is unsupported")]
    UnsupportedOperation,
    #[error("managed-WebView payload commitment algorithm is unsupported")]
    UnsupportedCommitmentAlgorithm,
    #[error("managed-WebView payload length does not match durable binding")]
    PayloadLengthMismatch,
    #[error("managed-WebView payload commitment could not be initialized")]
    CommitmentInitializationFailed,
    #[error("managed-WebView payload commitment does not match durable binding")]
    CommitmentMismatch,
}

pub fn verify_managed_web_payload_binding(
    key: &ManagedWebPayloadCommitmentKey,
    binding: &DurableManagedWebPayloadBinding,
    payload: &[u8],
) -> Result<(), ManagedWebPayloadVerificationError> {
    if binding.payload_ref.0.is_nil() {
        return Err(ManagedWebPayloadVerificationError::InvalidPayloadRef);
    }
    if !managed_web_payload_operation(binding.operation) {
        return Err(ManagedWebPayloadVerificationError::UnsupportedOperation);
    }
    if binding.commitment_algorithm != MANAGED_WEB_PAYLOAD_COMMITMENT_ALGORITHM {
        return Err(ManagedWebPayloadVerificationError::UnsupportedCommitmentAlgorithm);
    }
    if binding.payload_len != payload.len() as u64 {
        return Err(ManagedWebPayloadVerificationError::PayloadLengthMismatch);
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
        .map_err(|_| ManagedWebPayloadVerificationError::CommitmentInitializationFailed)?;
    update_commitment_mac(
        &mut mac,
        binding.action_id,
        binding.payload_ref,
        binding.operation,
        binding.payload_len,
        payload,
    );
    mac.verify_slice(&binding.commitment_digest)
        .map_err(|_| ManagedWebPayloadVerificationError::CommitmentMismatch)
}

impl ConsequentialJournal {
    /// Persist one payload commitment for an already-admitted managed-WebView
    /// payload-bearing action.
    ///
    /// Only payload-free metadata is written. The caller's actual text/key/scroll
    /// bytes remain process-local and must be verified again immediately before
    /// executor delivery.
    pub async fn record_managed_web_payload_binding(
        &self,
        queued: &CanonicalQueuedAction,
        key: &ManagedWebPayloadCommitmentKey,
        payload_ref: ManagedWebPayloadRef,
        payload: &[u8],
    ) -> Result<DurableManagedWebPayloadBinding, ConsequentialJournalError> {
        let action_id = queued.action.id;
        if queued.envelope.transport_action_id != action_id
            || queued.envelope.session_id != queued.action.session_id
            || payload_ref.0.is_nil()
        {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "managed_web_payload_binding_action_mismatch",
                current: None,
            });
        }

        let Some(operation) = self.admitted_operation(action_id).await? else {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "managed_web_payload_binding_without_operation",
                current: self.recovery_state(action_id).await,
            });
        };
        if !managed_web_payload_operation(operation) {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "managed_web_payload_binding_unsupported_operation",
                current: self.recovery_state(action_id).await,
            });
        }

        let state = self.state.lock().await;
        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::Admitted) {
            return Err(ConsequentialJournalError::InvalidTransition {
                action_id,
                attempted: "managed_web_payload_binding_after_authorization",
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
                attempted: "managed_web_payload_binding_without_exact_intent",
                current,
            })?;

        let payload_len = u64::try_from(payload.len()).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: format!(
                    "managed-WebView payload length could not be represented: {error}"
                ),
            }
        })?;
        let commitment_digest = compute_commitment(
            key,
            action_id,
            payload_ref,
            operation,
            payload_len,
            payload,
        )?;
        let binding = DurableManagedWebPayloadBinding {
            action_id,
            intent_journal_sequence: intent_entry.journal_sequence,
            payload_ref,
            operation,
            payload_len,
            commitment_algorithm: MANAGED_WEB_PAYLOAD_COMMITMENT_ALGORITHM.into(),
            commitment_digest,
        };
        let encoded = serde_json::to_vec(&binding).map_err(|error| {
            ConsequentialJournalError::Serialization {
                message: error.to_string(),
            }
        })?;
        let journal_path = Arc::clone(&self.path);
        let binding_path = managed_web_payload_binding_path(journal_path.as_path(), action_id);

        // Hold the journal state lock until the companion record is durable so
        // authorization cannot be sequenced ahead of the payload commitment.
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

    pub async fn managed_web_payload_binding(
        &self,
        action_id: Uuid,
    ) -> Result<Option<DurableManagedWebPayloadBinding>, ConsequentialJournalError> {
        let Some(operation) = self.admitted_operation(action_id).await? else {
            return Ok(None);
        };
        if !managed_web_payload_operation(operation) {
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

        let binding_path = managed_web_payload_binding_path(journal_path.as_path(), action_id);
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
            || binding.operation != operation
            || !managed_web_payload_operation(binding.operation)
            || binding.commitment_algorithm != MANAGED_WEB_PAYLOAD_COMMITMENT_ALGORITHM
            || binding.commitment_digest.len() != MANAGED_WEB_PAYLOAD_COMMITMENT_BYTES
        {
            return Err(ConsequentialJournalError::Serialization {
                message: "managed-WebView payload binding does not match admitted intent".into(),
            });
        }
        Ok(Some(binding))
    }
}

fn managed_web_payload_operation(operation: CanonicalActionOperation) -> bool {
    matches!(
        operation,
        CanonicalActionOperation::InputText
            | CanonicalActionOperation::KeyInput
            | CanonicalActionOperation::Scroll
    )
}

fn operation_tag(operation: CanonicalActionOperation) -> Option<u8> {
    match operation {
        CanonicalActionOperation::InputText => Some(0x01),
        CanonicalActionOperation::KeyInput => Some(0x02),
        CanonicalActionOperation::Scroll => Some(0x03),
        _ => None,
    }
}

fn compute_commitment(
    key: &ManagedWebPayloadCommitmentKey,
    action_id: Uuid,
    payload_ref: ManagedWebPayloadRef,
    operation: CanonicalActionOperation,
    payload_len: u64,
    payload: &[u8],
) -> Result<Vec<u8>, ConsequentialJournalError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).map_err(|_| {
        ConsequentialJournalError::Serialization {
            message: "managed-WebView payload commitment could not be initialized".into(),
        }
    })?;
    update_commitment_mac(
        &mut mac,
        action_id,
        payload_ref,
        operation,
        payload_len,
        payload,
    );
    Ok(mac.finalize().into_bytes().to_vec())
}

fn update_commitment_mac(
    mac: &mut Hmac<Sha256>,
    action_id: Uuid,
    payload_ref: ManagedWebPayloadRef,
    operation: CanonicalActionOperation,
    payload_len: u64,
    payload: &[u8],
) {
    mac.update(MANAGED_WEB_PAYLOAD_COMMITMENT_DOMAIN);
    mac.update(action_id.as_bytes());
    mac.update(payload_ref.0.as_bytes());
    mac.update(&[operation_tag(operation).unwrap_or(0xff)]);
    mac.update(&payload_len.to_be_bytes());
    mac.update(payload);
}

fn managed_web_payload_binding_path(journal_path: &Path, action_id: Uuid) -> PathBuf {
    PathBuf::from(format!(
        "{}.managed-web-payload-{action_id}.json",
        journal_path.display()
    ))
}

fn write_binding_create_new(
    path: &Path,
    encoded: &[u8],
) -> Result<(), ConsequentialJournalError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "create_managed_web_payload_binding",
            message: error.to_string(),
        })?;
    file.write_all(encoded)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "write_managed_web_payload_binding",
            message: error.to_string(),
        })?;
    file.flush()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "flush_managed_web_payload_binding",
            message: error.to_string(),
        })?;
    file.sync_all()
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "sync_managed_web_payload_binding",
            message: error.to_string(),
        })?;
    Ok(())
}

fn read_binding(
    path: &Path,
) -> Result<Option<DurableManagedWebPayloadBinding>, ConsequentialJournalError> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ConsequentialJournalError::Io {
                operation: "open_managed_web_payload_binding",
                message: error.to_string(),
            });
        }
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| ConsequentialJournalError::Io {
            operation: "read_managed_web_payload_binding",
            message: error.to_string(),
        })?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| ConsequentialJournalError::Serialization {
            message: error.to_string(),
        })
}
