use localview_live_bridge::DispatchExecutionPermit;
use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    WindowsUiaDispatchContextBlocker, WindowsUiaDispatchContextObservation,
    WindowsUiaDispatchContextRequirements, evaluate_windows_uia_dispatch_context,
};

pub const MAX_VERIFIED_KEY_EVENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsKeyTransition {
    KeyDown,
    KeyUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsVerifiedKeyEvent {
    pub virtual_key: u16,
    pub transition: WindowsKeyTransition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVerifiedKeyboardBatch {
    events: Vec<WindowsVerifiedKeyEvent>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsVerifiedKeyboardBatchError {
    #[error("verified keyboard batch must contain at least one event")]
    EmptyBatch,
    #[error("verified keyboard batch contains {actual} events; maximum is {maximum}")]
    TooManyEvents { actual: usize, maximum: usize },
    #[error("verified keyboard batch contains virtual-key 0 at event index {index}")]
    InvalidVirtualKey { index: usize },
}

impl WindowsVerifiedKeyboardBatch {
    pub fn new(
        events: Vec<WindowsVerifiedKeyEvent>,
    ) -> Result<Self, WindowsVerifiedKeyboardBatchError> {
        if events.is_empty() {
            return Err(WindowsVerifiedKeyboardBatchError::EmptyBatch);
        }
        if events.len() > MAX_VERIFIED_KEY_EVENTS {
            return Err(WindowsVerifiedKeyboardBatchError::TooManyEvents {
                actual: events.len(),
                maximum: MAX_VERIFIED_KEY_EVENTS,
            });
        }
        if let Some(index) = events.iter().position(|event| event.virtual_key == 0) {
            return Err(WindowsVerifiedKeyboardBatchError::InvalidVirtualKey { index });
        }
        Ok(Self { events })
    }

    pub fn events(&self) -> &[WindowsVerifiedKeyEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputRequest {
    pub(crate) dispatch_attempt_ref: Uuid,
    pub(crate) action_id: Uuid,
    pub(crate) preparation_journal_sequence: u64,
    pub(crate) preparation_receipt_ref: String,
    pub(crate) snapshot_cut_ref: String,
    pub(crate) provider_incarnation_ref: ProviderIncarnationRef,
    pub(crate) target_incarnation_ref: TargetIncarnationRef,
    pub(crate) element_ref: ProviderElementRef,
    pub(crate) context_requirements: WindowsUiaDispatchContextRequirements,
    pub(crate) batch_digest: String,
    pub(crate) batch: WindowsVerifiedKeyboardBatch,
}

impl WindowsUiaVerifiedInputRequest {
    pub fn from_execution_permit(
        permit: &DispatchExecutionPermit,
        snapshot_cut_ref: String,
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        element_ref: ProviderElementRef,
        context_requirements: WindowsUiaDispatchContextRequirements,
        batch: WindowsVerifiedKeyboardBatch,
    ) -> Self {
        let batch_digest = verified_keyboard_batch_digest(&batch);
        Self {
            dispatch_attempt_ref: Uuid::new_v4(),
            action_id: permit.action_id(),
            preparation_journal_sequence: permit.preparation_journal_sequence(),
            preparation_receipt_ref: permit.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref,
            provider_incarnation_ref,
            target_incarnation_ref,
            element_ref,
            context_requirements,
            batch_digest,
            batch,
        }
    }

    pub fn dispatch_attempt_ref(&self) -> Uuid {
        self.dispatch_attempt_ref
    }
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }
    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }
    pub fn preparation_receipt_ref(&self) -> &str {
        &self.preparation_receipt_ref
    }
    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }
    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }
    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }
    pub fn element_ref(&self) -> &ProviderElementRef {
        &self.element_ref
    }
    pub fn context_requirements(&self) -> WindowsUiaDispatchContextRequirements {
        self.context_requirements
    }
    pub fn batch_digest(&self) -> &str {
        &self.batch_digest
    }
    pub fn batch(&self) -> &WindowsVerifiedKeyboardBatch {
        &self.batch
    }
}

fn verified_keyboard_batch_digest(batch: &WindowsVerifiedKeyboardBatch) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"localview.windows.verified-input-batch.v1\0");
    for event in batch.events() {
        hasher.update(event.virtual_key.to_le_bytes());
        hasher.update([match event.transition {
            WindowsKeyTransition::KeyDown => 0_u8,
            WindowsKeyTransition::KeyUp => 1_u8,
        }]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsKeyboardStateSnapshot {
    pub shift_down: bool,
    pub control_down: bool,
    pub alt_down: bool,
    pub left_windows_down: bool,
    pub right_windows_down: bool,
    pub caps_lock_on: bool,
    pub num_lock_on: bool,
    pub scroll_lock_on: bool,
    pub layout_identity: Option<String>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsInputDispatchBlocker {
    #[error("current keyboard modifier state conflicts with verified input dispatch")]
    InputStateConflict,
    #[error("platform input backend returned inserted={inserted} for requested={requested}")]
    BackendResultInvalid { requested: u32, inserted: u32 },
}

pub fn evaluate_windows_keyboard_state(
    state: &WindowsKeyboardStateSnapshot,
) -> Result<(), WindowsInputDispatchBlocker> {
    if state.shift_down
        || state.control_down
        || state.alt_down
        || state.left_windows_down
        || state.right_windows_down
    {
        return Err(WindowsInputDispatchBlocker::InputStateConflict);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsInputInsertionClass {
    FullyInserted,
    PartialDispatchUnknownOutcome,
    ZeroInsertedBlocked,
}

pub fn classify_windows_input_insertion(
    requested: u32,
    inserted: u32,
) -> Result<WindowsInputInsertionClass, WindowsInputDispatchBlocker> {
    if requested == 0 || inserted > requested {
        return Err(WindowsInputDispatchBlocker::BackendResultInvalid {
            requested,
            inserted,
        });
    }
    if inserted == requested {
        return Ok(WindowsInputInsertionClass::FullyInserted);
    }
    if inserted == 0 {
        return Ok(WindowsInputInsertionClass::ZeroInsertedBlocked);
    }
    Ok(WindowsInputInsertionClass::PartialDispatchUnknownOutcome)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInputInsertRawResult {
    pub requested_event_count: u32,
    pub inserted_event_count: u32,
    pub raw_error_code: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVerifiedInputBoundaryReceipt {
    pub dispatch_context: WindowsUiaDispatchContextObservation,
    pub keyboard_state: WindowsKeyboardStateSnapshot,
    pub requested_event_count: u32,
    pub inserted_event_count: u32,
    pub insertion_class: WindowsInputInsertionClass,
    pub raw_error_code: Option<u32>,
    pub reconciliation_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputReceipt {
    dispatch_attempt_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    snapshot_cut_ref: String,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    element_ref: ProviderElementRef,
    batch_digest: String,
    boundary: WindowsVerifiedInputBoundaryReceipt,
}

impl WindowsUiaVerifiedInputReceipt {
    #[cfg(windows)]
    pub(crate) fn from_request(
        request: WindowsUiaVerifiedInputRequest,
        boundary: WindowsVerifiedInputBoundaryReceipt,
    ) -> Self {
        Self {
            dispatch_attempt_ref: request.dispatch_attempt_ref,
            action_id: request.action_id,
            preparation_journal_sequence: request.preparation_journal_sequence,
            preparation_receipt_ref: request.preparation_receipt_ref,
            snapshot_cut_ref: request.snapshot_cut_ref,
            provider_incarnation_ref: request.provider_incarnation_ref,
            target_incarnation_ref: request.target_incarnation_ref,
            element_ref: request.element_ref,
            batch_digest: request.batch_digest,
            boundary,
        }
    }

    pub fn dispatch_attempt_ref(&self) -> Uuid {
        self.dispatch_attempt_ref
    }
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }
    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }
    pub fn preparation_receipt_ref(&self) -> &str {
        &self.preparation_receipt_ref
    }
    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }
    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }
    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }
    pub fn element_ref(&self) -> &ProviderElementRef {
        &self.element_ref
    }
    pub fn batch_digest(&self) -> &str {
        &self.batch_digest
    }
    pub fn boundary(&self) -> &WindowsVerifiedInputBoundaryReceipt {
        &self.boundary
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsVerifiedInputBoundaryError {
    #[error("verified input environment could not observe final dispatch context")]
    ContextObservationFailed,
    #[error("verified input final dispatch context is blocked: {0}")]
    ContextBlocked(WindowsUiaDispatchContextBlocker),
    #[error("verified input environment could not snapshot keyboard state")]
    KeyboardStateObservationFailed,
    #[error("verified input final keyboard state conflicts with the authorized batch")]
    InputStateConflict,
    #[error(
        "verified input backend reported requested={reported} but the authorized batch contains {expected} events"
    )]
    RequestedCountMismatch { expected: u32, reported: u32 },
    #[error("verified input backend returned an invalid insertion count: {0}")]
    InvalidInsertionResult(WindowsInputDispatchBlocker),
}

/// Narrow environment abstraction used to keep the final volatile observations
/// and the single platform insertion attempt in one explicit ordering boundary.
/// Production implementations are responsible for collecting these observations
/// from the live Windows target; tests can exercise the authority semantics
/// without performing any OS side effect.
pub trait WindowsVerifiedInputEnvironment {
    fn observe_dispatch_context(
        &mut self,
    ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError>;

    fn snapshot_keyboard_state(
        &mut self,
    ) -> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError>;

    fn insert_events(&mut self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult;
}

/// Execute one verified keyboard boundary in fail-closed order:
/// final context -> keyboard state -> one insertion attempt -> classification.
/// No insertion occurs when either volatile fence is blocked.
pub fn execute_windows_verified_input_boundary<E>(
    requirements: WindowsUiaDispatchContextRequirements,
    batch: &WindowsVerifiedKeyboardBatch,
    environment: &mut E,
) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsVerifiedInputBoundaryError>
where
    E: WindowsVerifiedInputEnvironment,
{
    let dispatch_context = environment.observe_dispatch_context()?;
    evaluate_windows_uia_dispatch_context(requirements, &dispatch_context)
        .map_err(WindowsVerifiedInputBoundaryError::ContextBlocked)?;

    let keyboard_state = environment.snapshot_keyboard_state()?;
    evaluate_windows_keyboard_state(&keyboard_state).map_err(|error| match error {
        WindowsInputDispatchBlocker::InputStateConflict => {
            WindowsVerifiedInputBoundaryError::InputStateConflict
        }
        other => WindowsVerifiedInputBoundaryError::InvalidInsertionResult(other),
    })?;

    let raw = environment.insert_events(batch.events());
    let expected = batch.len() as u32;
    if raw.requested_event_count != expected {
        return Err(WindowsVerifiedInputBoundaryError::RequestedCountMismatch {
            expected,
            reported: raw.requested_event_count,
        });
    }
    let insertion_class = classify_windows_input_insertion(expected, raw.inserted_event_count)
        .map_err(WindowsVerifiedInputBoundaryError::InvalidInsertionResult)?;

    Ok(WindowsVerifiedInputBoundaryReceipt {
        dispatch_context,
        keyboard_state,
        requested_event_count: expected,
        inserted_event_count: raw.inserted_event_count,
        insertion_class,
        raw_error_code: raw.raw_error_code,
        reconciliation_required: true,
    })
}
