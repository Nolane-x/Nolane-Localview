use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

static VISUAL_PERMISSION_CHECK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Current knowledge about permission to obtain visual observations.
///
/// This authority domain is intentionally separate from Accessibility semantic
/// control. A visual grant does not imply AX trust, and AX trust does not imply
/// visual-observation permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualObservationPermissionState {
    Granted,
    Denied,
    Unknown,
}

/// Immutable provider-owned observation of visual-observation permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualObservationPermissionRevision {
    state: VisualObservationPermissionState,
    check_sequence: u64,
}

impl VisualObservationPermissionRevision {
    const fn observed(state: VisualObservationPermissionState, check_sequence: u64) -> Self {
        Self {
            state,
            check_sequence,
        }
    }

    pub const fn state(&self) -> VisualObservationPermissionState {
        self.state
    }

    pub const fn check_sequence(&self) -> u64 {
        self.check_sequence
    }
}

/// Opaque authority token for visual observation only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualObservationPermit {
    permission_check_sequence: u64,
}

impl VisualObservationPermit {
    pub const fn permission_check_sequence(&self) -> u64 {
        self.permission_check_sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VisualObservationPermissionError {
    #[error("visual-observation permission is required")]
    PermissionRequired,
    #[error("visual-observation permission state is unknown")]
    PermissionUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualObservationOutcome {
    Authorized(VisualObservationPermit),
    Denied(VisualObservationPermissionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualObservationDecision {
    revision: VisualObservationPermissionRevision,
    outcome: VisualObservationOutcome,
}

impl VisualObservationDecision {
    pub const fn revision(&self) -> VisualObservationPermissionRevision {
        self.revision
    }

    pub const fn outcome(&self) -> VisualObservationOutcome {
        self.outcome
    }

    pub const fn permit(&self) -> Option<VisualObservationPermit> {
        match self.outcome {
            VisualObservationOutcome::Authorized(permit) => Some(permit),
            VisualObservationOutcome::Denied(_) => None,
        }
    }

    pub const fn denial(&self) -> Option<VisualObservationPermissionError> {
        match self.outcome {
            VisualObservationOutcome::Authorized(_) => None,
            VisualObservationOutcome::Denied(error) => Some(error),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VisualObservationPermissionProvider;

impl VisualObservationPermissionProvider {
    pub const fn new() -> Self {
        Self
    }

    pub fn current_permission_revision(&self) -> VisualObservationPermissionRevision {
        permission_revision(platform_visual_permission_state())
    }

    pub fn observation_decision(&self) -> VisualObservationDecision {
        decision_from_revision(self.current_permission_revision())
    }
}

fn permission_revision(
    state: VisualObservationPermissionState,
) -> VisualObservationPermissionRevision {
    let check_sequence = VISUAL_PERMISSION_CHECK_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    VisualObservationPermissionRevision::observed(state, check_sequence)
}

fn decision_from_revision(
    revision: VisualObservationPermissionRevision,
) -> VisualObservationDecision {
    let outcome = match revision.state {
        VisualObservationPermissionState::Granted => {
            VisualObservationOutcome::Authorized(VisualObservationPermit {
                permission_check_sequence: revision.check_sequence,
            })
        }
        VisualObservationPermissionState::Denied => VisualObservationOutcome::Denied(
            VisualObservationPermissionError::PermissionRequired,
        ),
        VisualObservationPermissionState::Unknown => VisualObservationOutcome::Denied(
            VisualObservationPermissionError::PermissionUnknown,
        ),
    };

    VisualObservationDecision { revision, outcome }
}

// The real macOS OS-backed observation is intentionally added only after the
// dedicated real-provider RED proves the missing binding. Until then this
// authority fails closed rather than inferring permission from AX trust.
fn platform_visual_permission_state() -> VisualObservationPermissionState {
    VisualObservationPermissionState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granted_visual_revision_mints_only_visual_authority() {
        let revision = permission_revision(VisualObservationPermissionState::Granted);
        let decision = decision_from_revision(revision);
        let permit = decision.permit().expect("granted visual permission must mint visual authority");
        assert_eq!(permit.permission_check_sequence(), revision.check_sequence());
    }

    #[test]
    fn denied_and_unknown_visual_revisions_fail_closed() {
        for state in [
            VisualObservationPermissionState::Denied,
            VisualObservationPermissionState::Unknown,
        ] {
            let decision = decision_from_revision(permission_revision(state));
            assert!(decision.permit().is_none());
        }
    }
}
