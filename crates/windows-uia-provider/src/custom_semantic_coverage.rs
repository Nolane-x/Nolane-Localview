use localview_native_provider::NativeSemanticNodeObservation;

use crate::{WindowsUiaActionCapabilities, WindowsUiaPattern, WindowsUiaPatternSupport};

pub const WINDOWS_UIA_CUSTOM_SEMANTIC_COVERAGE_ATTRIBUTE: &str =
    "windows_uia.custom_semantic_coverage";
pub const WINDOWS_UIA_ACCESSIBILITY_PARTIAL_CUSTOM_CONTROL_DEBT: &str =
    "uia_accessibility_partial_custom_control";
pub const WINDOWS_UIA_ACCESSIBILITY_OPAQUE_CUSTOM_CONTROL_DEBT: &str =
    "uia_accessibility_opaque_custom_control";
const WINDOWS_UIA_CUSTOM_CONTROL_TYPE: &str = "uia_control_type:50025";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaCustomSemanticCoverage {
    CustomSemantic,
    CustomPartial,
    CustomOpaque,
}

impl WindowsUiaCustomSemanticCoverage {
    pub const fn as_wire_value(self) -> &'static str {
        match self {
            Self::CustomSemantic => "custom_semantic",
            Self::CustomPartial => "custom_partial",
            Self::CustomOpaque => "custom_opaque",
        }
    }

    pub const fn incompleteness_debt(self) -> Option<&'static str> {
        match self {
            Self::CustomSemantic => None,
            Self::CustomPartial => Some(WINDOWS_UIA_ACCESSIBILITY_PARTIAL_CUSTOM_CONTROL_DEBT),
            Self::CustomOpaque => Some(WINDOWS_UIA_ACCESSIBILITY_OPAQUE_CUSTOM_CONTROL_DEBT),
        }
    }
}

pub(crate) fn classify_custom_semantic_coverage(
    node: &NativeSemanticNodeObservation,
    has_observed_child: bool,
    traversal_complete: bool,
) -> Option<WindowsUiaCustomSemanticCoverage> {
    if node.control_type.as_deref() != Some(WINDOWS_UIA_CUSTOM_CONTROL_TYPE) {
        return None;
    }

    let capabilities = WindowsUiaActionCapabilities::from_node(node);
    let has_supported_pattern = WindowsUiaPattern::ALL.into_iter().any(|pattern| {
        capabilities.support_for(pattern) == WindowsUiaPatternSupport::Supported
    });
    if has_observed_child || has_supported_pattern {
        return Some(WindowsUiaCustomSemanticCoverage::CustomSemantic);
    }

    // Absence of children/actions is only meaningful after a complete bounded
    // traversal. If resource limits truncated the tree, the existing resource
    // debt already keeps the snapshot incomplete and we must not misdiagnose
    // provider weakness from missing observations.
    if !traversal_complete {
        return None;
    }

    let has_identity_metadata = [
        node.name.as_deref(),
        node.automation_id.as_deref(),
        node.class_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.trim().is_empty());

    Some(if has_identity_metadata {
        WindowsUiaCustomSemanticCoverage::CustomPartial
    } else {
        WindowsUiaCustomSemanticCoverage::CustomOpaque
    })
}
