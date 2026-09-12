use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
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
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub batch: WindowsVerifiedKeyboardBatch,
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
