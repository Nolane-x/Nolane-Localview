use localview_protocol::{
    DispatchResult, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
    TransportResult,
};
use uuid::Uuid;

use crate::{
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
};

/// Exact provider-side semantic method selected by the trusted runtime from the
/// durably admitted canonical operation. This is deliberately distinct from the
/// capability pattern: Expand and Collapse share one UIA pattern but are not the
/// same side effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaPatternDispatchOperation {
    Invoke,
    Select,
    Toggle,
    Expand,
    Collapse,
}

impl WindowsUiaPatternDispatchOperation {
    pub const fn required_pattern(self) -> WindowsUiaPattern {
        match self {
            Self::Invoke => WindowsUiaPattern::Invoke,
            Self::Select => WindowsUiaPattern::SelectionItem,
            Self::Toggle => WindowsUiaPattern::Toggle,
            Self::Expand | Self::Collapse => WindowsUiaPattern::ExpandCollapse,
        }
    }
}

/// One exact provider-side dispatch attempt. These fields deliberately repeat
/// the runtime authority binding so the MTA worker can reject cross-cut,
/// cross-incarnation, cross-element, cross-operation, or replayed execution
/// attempts before it touches a live UIA pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaPatternDispatchRequest {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
    pub dispatch_operation: WindowsUiaPatternDispatchOperation,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
}

/// Provider-owned dispatch evidence. `DispatchedFull` is only evidence that the
/// UIA call reached the exact live provider pattern; it never claims the expected
/// world/postcondition was independently verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaPatternDispatchReceipt {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
    pub dispatch_operation: WindowsUiaPatternDispatchOperation,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub final_context: WindowsUiaDispatchContextObservation,
    pub transport_result: TransportResult,
    pub dispatch_result: DispatchResult,
}
