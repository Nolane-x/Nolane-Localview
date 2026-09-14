use localview_resource_governor::{
    ResourceAdmissionDenial, ResourceReservation, ResourceWorkKind, RuntimeResourceGovernor,
};
use thiserror::Error;

use crate::{
    AxElementIdentity, AxUnresponsiveElementBinding, VisualObservationDecision,
    VisualObservationOutcome, VisualObservationPermissionError,
};

/// Why a higher-cost observation path was requested.
///
/// The reason is diagnostic/planning input only. It is deliberately not an
/// authority token and cannot by itself authorize visual capture or input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxVisualEscalationReason {
    ProviderTimeout,
}

/// A bounded request created after a consumed AX binding reports an
/// inconclusive timeout/unresponsive outcome.
///
/// The request owns the M04 unresponsive tombstone so escalation cannot revive
/// the old AX binding. It can later return that tombstone for normal semantic
/// reconciliation. No visual or input authority exists in this type.
#[derive(Debug, PartialEq, Eq)]
pub struct AxBoundedVisualEscalationRequest {
    unresponsive: AxUnresponsiveElementBinding,
}

impl AxBoundedVisualEscalationRequest {
    pub const fn reason(&self) -> AxVisualEscalationReason {
        AxVisualEscalationReason::ProviderTimeout
    }

    pub const fn invalidated_binding_sequence(&self) -> u64 {
        self.unresponsive.invalidated_binding_sequence()
    }

    pub fn target_identity(&self) -> &AxElementIdentity {
        self.unresponsive.identity()
    }

    /// M10 permits at most one targeted visual region from one timeout request.
    /// A broader/full-surface observation requires a separate planner decision.
    pub const fn max_visual_regions(&self) -> u8 {
        1
    }

    /// A request is not a capture permit. Authorization is a separate gate.
    pub const fn visual_capture_permitted(&self) -> bool {
        false
    }

    /// Provider timeout never upgrades observation into synthesized input.
    pub const fn input_fallback_permitted(&self) -> bool {
        false
    }

    pub fn into_unresponsive(self) -> AxUnresponsiveElementBinding {
        self.unresponsive
    }
}

#[derive(Debug, Error)]
pub enum AxVisualEscalationAuthorizationError {
    #[error("resource governor denied bounded visual escalation")]
    ResourceDenied(ResourceAdmissionDenial),
    #[error(transparent)]
    VisualPermission(VisualObservationPermissionError),
}

/// Opaque M10 permit for exactly one bounded visual-observation escalation.
///
/// The provider-owned resource reservation remains alive inside this value for
/// the permit lifetime. Dropping the permit releases the reservation. The M04
/// timeout tombstone remains owned by the permit, so no stale AX authority is
/// revived while visual evidence is collected.
#[derive(Debug)]
pub struct AxBoundedVisualEscalationPermit {
    request: AxBoundedVisualEscalationRequest,
    visual_permission_check_sequence: u64,
    _resource_reservation: ResourceReservation,
}

impl AxBoundedVisualEscalationPermit {
    pub const fn reason(&self) -> AxVisualEscalationReason {
        self.request.reason()
    }

    pub const fn invalidated_binding_sequence(&self) -> u64 {
        self.request.invalidated_binding_sequence()
    }

    pub fn target_identity(&self) -> &AxElementIdentity {
        self.request.target_identity()
    }

    pub const fn max_visual_regions(&self) -> u8 {
        self.request.max_visual_regions()
    }

    pub const fn visual_capture_permitted(&self) -> bool {
        true
    }

    pub const fn input_fallback_permitted(&self) -> bool {
        false
    }

    pub const fn visual_permission_check_sequence(&self) -> u64 {
        self.visual_permission_check_sequence
    }

    pub fn into_unresponsive(self) -> AxUnresponsiveElementBinding {
        self.request.into_unresponsive()
    }
}

/// Provider-owned M10 boundary for converting an explicit unresponsive outcome
/// into a bounded request and, only after independent visual-permission plus
/// resource-governor admission, into one opaque visual-observation permit.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxBoundedVisualEscalationAuthority;

impl AxBoundedVisualEscalationAuthority {
    pub const fn new() -> Self {
        Self
    }

    pub fn request_after_unresponsive(
        &self,
        unresponsive: AxUnresponsiveElementBinding,
    ) -> AxBoundedVisualEscalationRequest {
        AxBoundedVisualEscalationRequest { unresponsive }
    }

    pub fn authorize_visual_observation(
        &self,
        request: AxBoundedVisualEscalationRequest,
        visual_decision: VisualObservationDecision,
        governor: &RuntimeResourceGovernor,
        session_id: impl Into<String>,
        request_id: impl Into<String>,
    ) -> Result<AxBoundedVisualEscalationPermit, AxVisualEscalationAuthorizationError> {
        let reservation = governor
            .reserve(
                session_id,
                request_id,
                ResourceWorkKind::NativeVisualCapture,
            )
            .map_err(AxVisualEscalationAuthorizationError::ResourceDenied)?;

        let visual_permit = match visual_decision.outcome() {
            VisualObservationOutcome::Authorized(permit) => permit,
            VisualObservationOutcome::Denied(error) => {
                return Err(AxVisualEscalationAuthorizationError::VisualPermission(error));
            }
        };

        Ok(AxBoundedVisualEscalationPermit {
            request,
            visual_permission_check_sequence: visual_permit.permission_check_sequence(),
            _resource_reservation: reservation,
        })
    }
}
