use std::ops::Deref;

use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
use thiserror::Error;

/// Which volatile Windows facts must still be true at the immediate dispatch
/// boundary. Requirements are explicit so a semantic provider action does not
/// accidentally claim focus evidence it never needed or observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsUiaDispatchContextRequirements {
    pub require_foreground_target: bool,
    pub require_exact_element_focus: bool,
    pub require_no_modal_blocker: bool,
}

/// Point-in-time facts observed from Win32/UIA. Production code constructs the
/// target fields from the already-authorized attachment; callers do not get to
/// redefine the target by supplying these values to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchContextObservation {
    pub target_window_handle: u64,
    pub target_process_id: u32,
    pub foreground_window_handle: Option<u64>,
    pub foreground_process_id: Option<u32>,
    /// `Some(true)` means UIA proved the current focused element is the exact
    /// retained live element. `None` is explicit unknown / not observed.
    pub exact_element_focused: Option<bool>,
    /// Owned active popup/modal HWND that blocks direct dispatch to the target.
    pub modal_blocker_window_handle: Option<u64>,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchContextBlocker {
    #[error("dispatch target window/process identity is invalid")]
    InvalidTargetIdentity,
    #[error("current Windows foreground identity is unavailable")]
    ForegroundUnavailable,
    #[error("current foreground window does not match the authorized target")]
    ForegroundWindowMismatch { expected: u64, actual: u64 },
    #[error("current foreground process identity is unavailable")]
    ForegroundProcessUnavailable,
    #[error("current foreground process does not match the authorized target")]
    ForegroundProcessMismatch { expected: u32, actual: u32 },
    #[error("current focused UIA element is unavailable")]
    FocusUnavailable,
    #[error("current focused UIA element is not the exact retained action element")]
    ExactElementFocusMismatch,
    #[error("an owned/modal popup blocks dispatch to the authorized target")]
    ModalBlockerPresent { window_handle: u64 },
}

/// Pure fail-closed evaluator for volatile dispatch facts.
///
/// This function is deliberately OS-independent so the same authority semantics
/// can be mutation/property tested cheaply. The Windows MTA worker is responsible
/// for collecting the observation from real Win32/UIA APIs immediately before a
/// future side-effect boundary.
pub fn evaluate_windows_uia_dispatch_context(
    requirements: WindowsUiaDispatchContextRequirements,
    observation: &WindowsUiaDispatchContextObservation,
) -> Result<(), WindowsUiaDispatchContextBlocker> {
    if observation.target_window_handle == 0 || observation.target_process_id == 0 {
        return Err(WindowsUiaDispatchContextBlocker::InvalidTargetIdentity);
    }

    if requirements.require_foreground_target {
        let foreground_window = observation
            .foreground_window_handle
            .ok_or(WindowsUiaDispatchContextBlocker::ForegroundUnavailable)?;
        if foreground_window != observation.target_window_handle {
            return Err(WindowsUiaDispatchContextBlocker::ForegroundWindowMismatch {
                expected: observation.target_window_handle,
                actual: foreground_window,
            });
        }

        let foreground_process = observation
            .foreground_process_id
            .ok_or(WindowsUiaDispatchContextBlocker::ForegroundProcessUnavailable)?;
        if foreground_process != observation.target_process_id {
            return Err(WindowsUiaDispatchContextBlocker::ForegroundProcessMismatch {
                expected: observation.target_process_id,
                actual: foreground_process,
            });
        }
    }

    if requirements.require_exact_element_focus {
        match observation.exact_element_focused {
            Some(true) => {}
            Some(false) => {
                return Err(WindowsUiaDispatchContextBlocker::ExactElementFocusMismatch)
            }
            None => return Err(WindowsUiaDispatchContextBlocker::FocusUnavailable),
        }
    }

    if requirements.require_no_modal_blocker {
        if let Some(window_handle) = observation.modal_blocker_window_handle {
            return Err(WindowsUiaDispatchContextBlocker::ModalBlockerPresent {
                window_handle,
            });
        }
    }

    Ok(())
}

/// Request used by the MTA worker to bind volatile context to the exact retained
/// element/cut. The worker validates the element lease before observing context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchContextRequest {
    pub snapshot_cut_ref: String,
    pub element_ref: ProviderElementRef,
    pub requirements: WindowsUiaDispatchContextRequirements,
}

/// Raw data-only receipt emitted by the semantic MTA after observing and checking
/// the volatile context. The canonical public worker wraps this in
/// `WindowsUiaBoundDispatchContextReceipt` so the exact checked requirements
/// cannot be separated from the evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchContextReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub observation: WindowsUiaDispatchContextObservation,
}

/// Authority-bearing public receipt. It binds the exact requirement set to the
/// exact raw MTA receipt while preserving ergonomic read-only access to the raw
/// receipt fields through `Deref`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaBoundDispatchContextReceipt {
    pub requirements: WindowsUiaDispatchContextRequirements,
    pub context: WindowsUiaDispatchContextReceipt,
}

impl Deref for WindowsUiaBoundDispatchContextReceipt {
    type Target = WindowsUiaDispatchContextReceipt;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl WindowsUiaBoundDispatchContextReceipt {
    pub fn into_context(self) -> WindowsUiaDispatchContextReceipt {
        self.context
    }
}
