use std::{fmt, str};

pub use localview_live_bridge::{SetValueMode, SetValuePayloadRef};
use localview_live_bridge::CanonicalActionOperation;
use localview_protocol::{
    DispatchResult, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
    TransportResult,
};
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
};

pub const MAX_SET_VALUE_UTF8_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsUiaSetValueDispatchRequestError {
    #[error("Windows UI Automation SetValue authority metadata is invalid")]
    InvalidAuthorityMetadata,
    #[error("Windows UI Automation SetValue element lineage is inconsistent")]
    ElementLineageMismatch,
    #[error("Windows UI Automation SetValue payload reference is invalid")]
    InvalidPayloadRef,
    #[error("Windows UI Automation SetValue payload exceeds 16 KiB")]
    PayloadTooLarge,
    #[error("Windows UI Automation SetValue replacement payload contains U+0000")]
    PayloadContainsNul,
    #[error("Windows UI Automation SetValue replacement payload is not valid UTF-8")]
    PayloadNotUtf8,
    #[error("Windows UI Automation ClearValue requires an empty process-local payload")]
    ClearValuePayloadMustBeEmpty,
}

/// Dedicated move-only SetValue command crossing into the Windows UIA MTA worker.
///
/// The plaintext buffer is intentionally private, non-Clone, non-serde and
/// zeroized on drop. Debug output is metadata-only and never formats plaintext.
pub struct WindowsUiaSetValueDispatchRequest {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    secret_utf8: Zeroizing<Vec<u8>>,
}

impl WindowsUiaSetValueDispatchRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dispatch_attempt_ref: Uuid,
        action_id: Uuid,
        preparation_journal_sequence: u64,
        preparation_receipt_ref: String,
        snapshot_cut_ref: String,
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        element_ref: ProviderElementRef,
        context_requirements: WindowsUiaDispatchContextRequirements,
        payload_ref: SetValuePayloadRef,
        mode: SetValueMode,
        secret_utf8: Vec<u8>,
    ) -> Result<Self, WindowsUiaSetValueDispatchRequestError> {
        if dispatch_attempt_ref.is_nil()
            || action_id.is_nil()
            || preparation_journal_sequence == 0
            || preparation_receipt_ref.trim().is_empty()
            || snapshot_cut_ref.trim().is_empty()
            || provider_incarnation_ref.as_str().trim().is_empty()
            || target_incarnation_ref.as_str().trim().is_empty()
        {
            return Err(WindowsUiaSetValueDispatchRequestError::InvalidAuthorityMetadata);
        }
        if payload_ref.0.is_nil() {
            return Err(WindowsUiaSetValueDispatchRequestError::InvalidPayloadRef);
        }
        if element_ref.provider_incarnation_ref != provider_incarnation_ref
            || element_ref.target_incarnation_ref != target_incarnation_ref
            || element_ref.acquisition_cut_ref != snapshot_cut_ref
        {
            return Err(WindowsUiaSetValueDispatchRequestError::ElementLineageMismatch);
        }
        if secret_utf8.len() > MAX_SET_VALUE_UTF8_BYTES {
            return Err(WindowsUiaSetValueDispatchRequestError::PayloadTooLarge);
        }
        if str::from_utf8(&secret_utf8).is_err() {
            return Err(WindowsUiaSetValueDispatchRequestError::PayloadNotUtf8);
        }
        match mode {
            SetValueMode::ReplaceValue => {
                if secret_utf8.contains(&0) {
                    return Err(WindowsUiaSetValueDispatchRequestError::PayloadContainsNul);
                }
            }
            SetValueMode::ClearValue => {
                if !secret_utf8.is_empty() {
                    return Err(
                        WindowsUiaSetValueDispatchRequestError::ClearValuePayloadMustBeEmpty,
                    );
                }
            }
        }

        Ok(Self {
            dispatch_attempt_ref,
            action_id,
            preparation_journal_sequence,
            preparation_receipt_ref,
            snapshot_cut_ref,
            provider_incarnation_ref,
            target_incarnation_ref,
            element_ref,
            context_requirements,
            payload_ref,
            mode,
            secret_utf8: Zeroizing::new(secret_utf8),
        })
    }

    pub fn secret_utf8_len(&self) -> usize {
        self.secret_utf8.len()
    }

    #[cfg(windows)]
    pub(crate) fn secret_utf8_str(
        &self,
    ) -> Result<&str, WindowsUiaSetValueDispatchRequestError> {
        str::from_utf8(&self.secret_utf8)
            .map_err(|_| WindowsUiaSetValueDispatchRequestError::PayloadNotUtf8)
    }
}

impl fmt::Debug for WindowsUiaSetValueDispatchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsUiaSetValueDispatchRequest")
            .field("dispatch_attempt_ref", &self.dispatch_attempt_ref)
            .field("action_id", &self.action_id)
            .field(
                "preparation_journal_sequence",
                &self.preparation_journal_sequence,
            )
            .field("preparation_receipt_ref", &self.preparation_receipt_ref)
            .field("snapshot_cut_ref", &self.snapshot_cut_ref)
            .field("provider_incarnation_ref", &self.provider_incarnation_ref)
            .field("target_incarnation_ref", &self.target_incarnation_ref)
            .field("element_ref", &self.element_ref)
            .field("context_requirements", &self.context_requirements)
            .field("payload_ref", &self.payload_ref)
            .field("mode", &self.mode)
            .field("secret_utf8_len", &self.secret_utf8.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaSetValueDispatchReceipt {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
    pub dispatch_operation: CanonicalActionOperation,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub final_context: WindowsUiaDispatchContextObservation,
    pub transport_result: TransportResult,
    pub dispatch_result: DispatchResult,
}
