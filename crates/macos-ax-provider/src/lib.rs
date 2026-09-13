//! macOS Accessibility permission authority boundary for LocalView V4.3.
//!
//! M01 establishes provider-owned live Accessibility trust observation before
//! semantic-control admission. M02 extends that authority boundary to dispatch:
//! a permit admitted under an earlier trusted revision never authorizes a
//! consequential AX dispatch by itself; dispatch rechecks live OS trust and
//! either refreshes authority or fails closed if permission was revoked.
//! Application attachment, AX snapshots, observers, concrete AX action
//! dispatch, and capture/input fallback remain outside this slice.

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

/// Immutable OS-backed observation of Accessibility permission at one check
/// sequence.
///
/// The constructor is intentionally private. External callers can inspect a
/// revision returned by [`AxPermissionProvider`], but cannot manufacture a
/// `Trusted` revision and feed it back as semantic-control authority.
///
/// ```compile_fail
/// use localview_macos_ax_provider::{AxPermissionRevision, AxPermissionState};
///
/// let _forged = AxPermissionRevision::observed(
///     AxPermissionState::Trusted,
///     u64::MAX,
///     false,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxPermissionRevision {
    state: AxPermissionState,
    check_sequence: u64,
    prompt_requested: bool,
}

impl AxPermissionRevision {
    const fn observed(
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
/// control. The token is intentionally opaque and cannot directly authorize a
/// dispatch; callers must submit it to [`AxPermissionProvider::semantic_dispatch_decision`]
/// for a fresh OS-backed permission fence.
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
    #[error("macOS Accessibility permission was revoked after semantic-control admission")]
    PermissionRevoked,
    #[error("macOS Accessibility permission state is unknown")]
    PermissionUnknown,
}

/// Result of one provider-owned semantic-control authority check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSemanticControlOutcome {
    Authorized(AxSemanticControlPermit),
    Denied(AxPermissionError),
}

/// One coherent permission observation plus the semantic-control admission
/// decision derived from that same observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticControlDecision {
    revision: AxPermissionRevision,
    outcome: AxSemanticControlOutcome,
}

impl AxSemanticControlDecision {
    pub const fn revision(&self) -> AxPermissionRevision {
        self.revision
    }

    pub const fn outcome(&self) -> AxSemanticControlOutcome {
        self.outcome
    }

    pub const fn permit(&self) -> Option<AxSemanticControlPermit> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(permit) => Some(permit),
            AxSemanticControlOutcome::Denied(_) => None,
        }
    }

    pub const fn denial(&self) -> Option<AxPermissionError> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(_) => None,
            AxSemanticControlOutcome::Denied(error) => Some(error),
        }
    }
}

/// Dispatch-time permission fence. It records which previously admitted
/// permission revision was presented, the newer live revision observed before
/// dispatch, and the refreshed or denied authority derived from that new
/// observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticDispatchDecision {
    admitted_permission_check_sequence: u64,
    revision: AxPermissionRevision,
    outcome: AxSemanticControlOutcome,
}

impl AxSemanticDispatchDecision {
    pub const fn admitted_permission_check_sequence(&self) -> u64 {
        self.admitted_permission_check_sequence
    }

    pub const fn revision(&self) -> AxPermissionRevision {
        self.revision
    }

    pub const fn outcome(&self) -> AxSemanticControlOutcome {
        self.outcome
    }

    pub const fn permit(&self) -> Option<AxSemanticControlPermit> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(permit) => Some(permit),
            AxSemanticControlOutcome::Denied(_) => None,
        }
    }

    pub const fn denial(&self) -> Option<AxPermissionError> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(_) => None,
            AxSemanticControlOutcome::Denied(error) => Some(error),
        }
    }
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
    /// The boolean records whether a caller requested prompting in a wider
    /// flow; it does not alter the OS observation or grant authority by itself.
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

    /// Takes a fresh OS-backed permission observation and derives semantic-
    /// control admission authority from that exact revision. Callers never
    /// submit a revision to this method, so a fabricated `Trusted` state cannot
    /// cross the authority boundary.
    pub fn semantic_control_decision(
        &self,
        prompt_requested: bool,
    ) -> AxSemanticControlDecision {
        decision_from_revision(self.current_permission_revision(prompt_requested))
    }

    /// Revalidates Accessibility trust immediately before semantic dispatch.
    ///
    /// The previously admitted permit is evidence that admission once
    /// succeeded; it is not dispatch authority. This method always reads a new
    /// OS-backed permission revision. If permission is still trusted, callers
    /// receive a refreshed permit bound to that newer revision. If permission
    /// is now untrusted, the old authority is invalidated with a typed
    /// [`AxPermissionError::PermissionRevoked`] denial and no fallback authority
    /// is minted.
    pub fn semantic_dispatch_decision(
        &self,
        admitted_permit: AxSemanticControlPermit,
    ) -> AxSemanticDispatchDecision {
        dispatch_decision_from_revision(
            admitted_permit,
            self.current_permission_revision(false),
        )
    }
}

fn decision_from_revision(revision: AxPermissionRevision) -> AxSemanticControlDecision {
    let outcome = match revision.state {
        AxPermissionState::Trusted => {
            AxSemanticControlOutcome::Authorized(AxSemanticControlPermit {
                permission_check_sequence: revision.check_sequence,
            })
        }
        AxPermissionState::Untrusted => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRequired)
        }
        AxPermissionState::Unknown => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionUnknown)
        }
    };

    AxSemanticControlDecision { revision, outcome }
}

fn dispatch_decision_from_revision(
    admitted_permit: AxSemanticControlPermit,
    revision: AxPermissionRevision,
) -> AxSemanticDispatchDecision {
    let outcome = match revision.state {
        AxPermissionState::Trusted => {
            AxSemanticControlOutcome::Authorized(AxSemanticControlPermit {
                permission_check_sequence: revision.check_sequence,
            })
        }
        AxPermissionState::Untrusted => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRevoked)
        }
        AxPermissionState::Unknown => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionUnknown)
        }
    };

    AxSemanticDispatchDecision {
        admitted_permission_check_sequence: admitted_permit.permission_check_sequence,
        revision,
        outcome,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_permission_is_typed_and_never_mints_a_permit() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Untrusted,
            1,
            false,
        ));

        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionRequired));
    }

    #[test]
    fn unknown_permission_is_distinct_from_explicit_denial() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Unknown,
            2,
            false,
        ));

        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionUnknown));
    }

    #[test]
    fn prompt_requested_does_not_override_untrusted_state() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Untrusted,
            3,
            true,
        ));

        assert!(decision.revision().prompt_requested());
        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionRequired));
    }

    #[test]
    fn trusted_permission_mints_a_permit_bound_to_the_observed_revision() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Trusted,
            41,
            false,
        ));

        let permit = decision.permit().expect("trusted observation should authorize");
        assert_eq!(permit.permission_check_sequence(), 41);
        assert_eq!(decision.denial(), None);
    }

    #[test]
    fn revoked_permission_invalidates_prior_permit_at_dispatch() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 50,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Untrusted, 51, false),
        );

        assert_eq!(dispatch.admitted_permission_check_sequence(), 50);
        assert_eq!(dispatch.revision().check_sequence(), 51);
        assert_eq!(dispatch.permit(), None);
        assert_eq!(dispatch.denial(), Some(AxPermissionError::PermissionRevoked));
    }

    #[test]
    fn still_trusted_dispatch_refreshes_authority_to_newer_revision() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 60,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Trusted, 61, false),
        );

        let refreshed = dispatch
            .permit()
            .expect("still-trusted dispatch should refresh authority");
        assert_eq!(dispatch.admitted_permission_check_sequence(), 60);
        assert_eq!(refreshed.permission_check_sequence(), 61);
        assert_eq!(dispatch.denial(), None);
    }

    #[test]
    fn unknown_dispatch_state_never_reuses_prior_permit() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 70,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Unknown, 71, false),
        );

        assert_eq!(dispatch.permit(), None);
        assert_eq!(dispatch.denial(), Some(AxPermissionError::PermissionUnknown));
    }
}
