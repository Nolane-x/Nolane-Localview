use crate::{AxElementIdentity, AxUnresponsiveElementBinding};

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

/// Provider-owned M10 boundary for converting an explicit unresponsive outcome
/// into a bounded *request*. This first transition never mints visual authority.
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
}
