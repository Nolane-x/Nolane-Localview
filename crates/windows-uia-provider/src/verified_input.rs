use thiserror::Error;

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
