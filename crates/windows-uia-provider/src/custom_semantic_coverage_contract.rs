use std::collections::BTreeMap;

use localview_native_provider::NativeSemanticNodeObservation;
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};

use crate::{
    WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1, WindowsUiaActionCapabilities,
    WindowsUiaCustomSemanticCoverage, WindowsUiaPattern, WindowsUiaPatternSupport,
    custom_semantic_coverage::classify_custom_semantic_coverage,
};

fn node(
    control_type: &str,
    name: Option<&str>,
    automation_id: Option<&str>,
    class_name: Option<&str>,
    supported_pattern: Option<WindowsUiaPattern>,
) -> NativeSemanticNodeObservation {
    let mut capabilities = WindowsUiaActionCapabilities::default();
    for pattern in WindowsUiaPattern::ALL {
        capabilities.record(
            pattern,
            if Some(pattern) == supported_pattern {
                WindowsUiaPatternSupport::Supported
            } else {
                WindowsUiaPatternSupport::Unsupported
            },
        );
    }
    let mut attributes = BTreeMap::new();
    capabilities.write_attributes(&mut attributes);
    assert_eq!(
        attributes
            .get(WindowsUiaActionCapabilities::profile_attribute_key())
            .map(String::as_str),
        Some(WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1)
    );

    NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:w14-contract"),
            target_incarnation_ref: TargetIncarnationRef::from("target:w14-contract"),
            opaque_provider_element_id: "uia-runtime:[14]".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("window:w14-contract".into()),
            acquisition_cut_ref: "cut:w14-contract".into(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("custom".into()),
        name: name.map(str::to_owned),
        control_type: Some(control_type.into()),
        automation_id: automation_id.map(str::to_owned),
        class_name: class_name.map(str::to_owned),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes,
    }
}

#[test]
fn custom_with_real_semantic_child_is_semantic_even_without_action_pattern() {
    let observed = node(
        "uia_control_type:50025",
        Some("canvas host"),
        Some("canvas-host"),
        Some("OwnerDrawn"),
        None,
    );
    assert_eq!(
        classify_custom_semantic_coverage(&observed, true, true),
        Some(WindowsUiaCustomSemanticCoverage::CustomSemantic)
    );
}

#[test]
fn custom_with_supported_action_pattern_is_semantic_without_child() {
    let observed = node(
        "uia_control_type:50025",
        Some("custom button"),
        Some("custom-button"),
        Some("OwnerDrawn"),
        Some(WindowsUiaPattern::Invoke),
    );
    assert_eq!(
        classify_custom_semantic_coverage(&observed, false, true),
        Some(WindowsUiaCustomSemanticCoverage::CustomSemantic)
    );
}

#[test]
fn identity_only_custom_leaf_is_partial_after_complete_traversal() {
    let observed = node(
        "uia_control_type:50025",
        Some("owner drawn surface"),
        Some("owner-drawn"),
        Some("OwnerDrawn"),
        None,
    );
    let coverage = classify_custom_semantic_coverage(&observed, false, true);
    assert_eq!(
        coverage,
        Some(WindowsUiaCustomSemanticCoverage::CustomPartial)
    );
    assert_eq!(
        coverage.and_then(WindowsUiaCustomSemanticCoverage::incompleteness_debt),
        Some("uia_accessibility_partial_custom_control")
    );
}

#[test]
fn metadata_free_custom_leaf_is_opaque_after_complete_traversal() {
    let observed = node("uia_control_type:50025", None, None, None, None);
    let coverage = classify_custom_semantic_coverage(&observed, false, true);
    assert_eq!(coverage, Some(WindowsUiaCustomSemanticCoverage::CustomOpaque));
    assert_eq!(
        coverage.and_then(WindowsUiaCustomSemanticCoverage::incompleteness_debt),
        Some("uia_accessibility_opaque_custom_control")
    );
}

#[test]
fn truncated_traversal_does_not_infer_custom_partial_from_missing_children() {
    let observed = node(
        "uia_control_type:50025",
        Some("maybe truncated"),
        Some("maybe-truncated"),
        Some("OwnerDrawn"),
        None,
    );
    assert_eq!(
        classify_custom_semantic_coverage(&observed, false, false),
        None,
        "resource truncation already carries debt and must not be misdiagnosed as provider weakness"
    );
}

#[test]
fn standard_control_is_outside_custom_coverage_classification() {
    let observed = node(
        "uia_control_type:50000",
        Some("button"),
        Some("button"),
        Some("Button"),
        None,
    );
    assert_eq!(
        classify_custom_semantic_coverage(&observed, false, true),
        None
    );
}
