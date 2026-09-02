use std::collections::BTreeMap;

use localview_native_provider::NativeSemanticNodeObservation;
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaPattern, WindowsUiaPatternSupport,
    WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1,
};

fn node(
    realization: ProviderElementRealization,
    attributes: BTreeMap<String, String>,
) -> NativeSemanticNodeObservation {
    NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia-capability-contract"),
            target_incarnation_ref: TargetIncarnationRef::from("target:uia-capability-contract"),
            opaque_provider_element_id: "uia-runtime:[42,7]".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("window:uia-capability-contract".into()),
            acquisition_cut_ref: "cut:uia-capability-contract".into(),
            realization,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("button".into()),
        name: Some("Save".into()),
        control_type: Some("uia_control_type:50000".into()),
        automation_id: Some("save".into()),
        class_name: Some("Button".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes,
    }
}

#[test]
fn absent_or_unversioned_capability_evidence_is_unknown_not_unsupported() {
    let empty = WindowsUiaActionCapabilities::from_node(&node(
        ProviderElementRealization::RealizedCurrent,
        BTreeMap::new(),
    ));
    assert_eq!(
        empty.support_for(WindowsUiaPattern::Invoke),
        WindowsUiaPatternSupport::Unknown
    );

    let unversioned = WindowsUiaActionCapabilities::from_node(&node(
        ProviderElementRealization::RealizedCurrent,
        BTreeMap::from([(
            WindowsUiaPattern::Invoke.attribute_key().into(),
            WindowsUiaPatternSupport::Supported.as_wire_value().into(),
        )]),
    ));
    assert_eq!(
        unversioned.support_for(WindowsUiaPattern::Invoke),
        WindowsUiaPatternSupport::Unknown,
        "pattern strings without the declared capability profile are not authority"
    );
}

#[test]
fn typed_pattern_capabilities_parse_only_the_declared_profile() {
    let attributes = BTreeMap::from([
        (
            WindowsUiaActionCapabilities::profile_attribute_key().into(),
            WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1.into(),
        ),
        (
            WindowsUiaPattern::Invoke.attribute_key().into(),
            WindowsUiaPatternSupport::Supported.as_wire_value().into(),
        ),
        (
            WindowsUiaPattern::SelectionItem.attribute_key().into(),
            WindowsUiaPatternSupport::Unsupported.as_wire_value().into(),
        ),
        (
            WindowsUiaPattern::Value.attribute_key().into(),
            WindowsUiaPatternSupport::Unknown.as_wire_value().into(),
        ),
    ]);
    let capabilities = WindowsUiaActionCapabilities::from_node(&node(
        ProviderElementRealization::RealizedCurrent,
        attributes,
    ));

    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::Invoke),
        WindowsUiaPatternSupport::Supported
    );
    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::SelectionItem),
        WindowsUiaPatternSupport::Unsupported
    );
    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::Value),
        WindowsUiaPatternSupport::Unknown
    );
    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::Toggle),
        WindowsUiaPatternSupport::Unknown
    );
}

#[test]
fn malformed_or_future_profile_evidence_fails_closed_to_unknown() {
    let malformed = BTreeMap::from([
        (
            WindowsUiaActionCapabilities::profile_attribute_key().into(),
            WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1.into(),
        ),
        (
            WindowsUiaPattern::Invoke.attribute_key().into(),
            "probably".into(),
        ),
    ]);
    let capabilities = WindowsUiaActionCapabilities::from_node(&node(
        ProviderElementRealization::RealizedCurrent,
        malformed,
    ));
    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::Invoke),
        WindowsUiaPatternSupport::Unknown
    );

    let future = BTreeMap::from([
        (
            WindowsUiaActionCapabilities::profile_attribute_key().into(),
            "windows-uia-action-capabilities-v999".into(),
        ),
        (
            WindowsUiaPattern::Invoke.attribute_key().into(),
            WindowsUiaPatternSupport::Supported.as_wire_value().into(),
        ),
    ]);
    let capabilities = WindowsUiaActionCapabilities::from_node(&node(
        ProviderElementRealization::RealizedCurrent,
        future,
    ));
    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::Invoke),
        WindowsUiaPatternSupport::Unknown
    );
}

#[test]
fn first_reversible_pattern_taxonomy_has_stable_attribute_keys() {
    let cases = [
        (WindowsUiaPattern::Invoke, "windows_uia.pattern.invoke"),
        (
            WindowsUiaPattern::SelectionItem,
            "windows_uia.pattern.selection_item",
        ),
        (WindowsUiaPattern::Value, "windows_uia.pattern.value"),
        (WindowsUiaPattern::Toggle, "windows_uia.pattern.toggle"),
        (
            WindowsUiaPattern::ExpandCollapse,
            "windows_uia.pattern.expand_collapse",
        ),
        (
            WindowsUiaPattern::ScrollItem,
            "windows_uia.pattern.scroll_item",
        ),
        (
            WindowsUiaPattern::VirtualizedItem,
            "windows_uia.pattern.virtualized_item",
        ),
    ];

    assert_eq!(WindowsUiaPattern::ALL.len(), cases.len());
    for (pattern, expected) in cases {
        assert_eq!(pattern.attribute_key(), expected);
    }
    assert_eq!(WindowsUiaPatternSupport::Supported.as_wire_value(), "supported");
    assert_eq!(WindowsUiaPatternSupport::Unsupported.as_wire_value(), "unsupported");
    assert_eq!(WindowsUiaPatternSupport::Unknown.as_wire_value(), "unknown");
}

#[test]
fn virtualization_capability_never_promotes_provider_realization_state() {
    let observation = node(
        ProviderElementRealization::RealizationRequired,
        BTreeMap::from([
            (
                WindowsUiaActionCapabilities::profile_attribute_key().into(),
                WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1.into(),
            ),
            (
                WindowsUiaPattern::VirtualizedItem.attribute_key().into(),
                WindowsUiaPatternSupport::Supported.as_wire_value().into(),
            ),
        ]),
    );
    let capabilities = WindowsUiaActionCapabilities::from_node(&observation);

    assert_eq!(
        capabilities.support_for(WindowsUiaPattern::VirtualizedItem),
        WindowsUiaPatternSupport::Supported
    );
    assert_eq!(
        observation.element_ref.realization,
        ProviderElementRealization::RealizationRequired,
        "VirtualizedItem pattern availability is capability evidence, not realization evidence"
    );
}
