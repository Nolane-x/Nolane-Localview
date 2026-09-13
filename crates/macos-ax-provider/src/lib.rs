//! macOS Accessibility permission authority boundary for LocalView V4.3.
//!
//! M01 is intentionally narrow: it observes whether the current LocalView
//! process is trusted as an Accessibility client and decides whether that
//! observation may mint semantic-control authority. It does not yet attach to
//! applications, build AX snapshots, install observers, dispatch actions, or
//! request capture/input fallbacks.

use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

static AX_PERMISSION_CHECK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Current knowledge about macOS Accessibility trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxPermissionState {
    Trusted,
    Untrusted,
    Unknown,
}

/// Immutable observation of Accessibility permission at one check sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxPermissionRevision {
    state: AxPermissionState,
    check_sequence: u64,
    prompt_requested: bool,
}

impl AxPermissionRevision {
    /// Constructs a revision from an observed permission state.
    ///
    /// `prompt_requested` is evidence about caller intent only. In particular,
    /// it never upgrades `Untrusted` or `Unknown` into `Trusted`.
    pub const fn observed(
        state: AxPermissionState,
        check_sequence: u64,
        prompt_requested: bool,
    ) -> Self {
        Self {
            state,
            check_sequence,
            prompt_requested,
        }
    }

    pub const fn state(&self) -> AxPermissionState {
        self.state
    }

    pub const fn check_sequence(&self) -> u64 {
        self.check_sequence
    }

    pub const fn prompt_requested(&self) -> bool {
        self.prompt_requested
    }
}

/// Capability token proving the permission revision that admitted AX semantic
/// control. Later macOS action slices can bind this sequence into stronger
/// application/observer/action authority instead of reusing a bare boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticControlPermit {
    permission_check_sequence: u64,
}

impl AxSemanticControlPermit {
    pub const fn permission_check_sequence(&self) -> u64 {
        self.permission_check_sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxPermissionError {
    #[error("macOS Accessibility permission is required")]
    PermissionRequired,
    #[error("macOS Accessibility permission state is unknown")]
    PermissionUnknown,
}

/// Production boundary for observing and authorizing macOS Accessibility use.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxPermissionProvider;

impl AxPermissionProvider {
    pub const fn new() -> Self {
        Self
    }

    /// Reads current process trust from the operating system.
    ///
    /// M01 deliberately does not display a system prompt. The boolean records
    /// whether a caller has requested prompting in a wider flow; it does not
    /// alter the OS observation or grant authority by itself.
    pub fn current_permission_revision(
        &self,
        prompt_requested: bool,
    ) -> AxPermissionRevision {
        let check_sequence = AX_PERMISSION_CHECK_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
        AxPermissionRevision::observed(
            platform_permission_state(),
            check_sequence,
            prompt_requested,
        )
    }

    /// Mints semantic-control authority only from an explicitly trusted
    /// revision. Absent and unknown permission stay distinct typed failures.
    pub fn authorize_semantic_control(
        &self,
        revision: &AxPermissionRevision,
    ) -> Result<AxSemanticControlPermit, AxPermissionError> {
        match revision.state {
            AxPermissionState::Trusted => Ok(AxSemanticControlPermit {
                permission_check_sequence: revision.check_sequence,
            }),
            AxPermissionState::Untrusted => Err(AxPermissionError::PermissionRequired),
            AxPermissionState::Unknown => Err(AxPermissionError::PermissionUnknown),
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_permission_state() -> AxPermissionState {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        // AXUIElement.h declares `Boolean AXIsProcessTrusted(void)`. Core
        // Foundation's Boolean is an unsigned byte, so bind it as `u8` rather
        // than relying on Rust `bool` FFI layout.
        fn AXIsProcessTrusted() -> u8;
    }

    if unsafe { AXIsProcessTrusted() } != 0 {
        AxPermissionState::Trusted
    } else {
        AxPermissionState::Untrusted
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_permission_state() -> AxPermissionState {
    // A non-macOS build cannot truthfully answer an AX trust question.
    AxPermissionState::Unknown
}
