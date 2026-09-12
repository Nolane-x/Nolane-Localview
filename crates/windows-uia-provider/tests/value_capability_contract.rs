use std::collections::BTreeMap;

use localview_native_provider::NativeSemanticNodeObservation;
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1, WindowsUiaActionCapabilities,
    WindowsUiaBooleanCapabilityFact, WindowsUiaPattern, WindowsUiaPatternSupport,
    WindowsUiaValueCapabilityFacts,
};

fn node(attributes: BTreeMap<String, String>) -> NativeSemanticNodeObservation {
    NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:value-capability"),
            target_incarnation_ref: TargetIncarnationRef::from("target:value-capability"),
            opaque_provider_element_id: "uia-runtime:[73,4]".into(),
            semantic_locator_hints: vec!["automation_id=value-capability".into()],
            parent_surface_ref: Some("window:value-capability".into()),
            acquisition_cut_ref: "cut:value-capability".into(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("edit".into()),
        name: Some("Value capability".into()),
        control_type: Some("uia_control_type:50004".into()),
        automation_id: Some("value-capability".into()),
        class_name: Some("Edit".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes,
    }
}

fn value_attributes(
    support: WindowsUiaPatternSupport,
    is_password: Option<&str>,
    is_read_only: Option<&str>,
) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::from([
        (
            WindowsUiaActionCapabilities::profile_attribute_key().into(),
            WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1.into(),
        ),
        (
            WindowsUiaPattern::Value.attribute_key().into(),
            support.as_wire_value().into(),
        ),
    ]);
    if let Some(value) = is_password {
        attributes.insert("windows_uia.is_password".into(), value.into());
    }
    if let Some(value) = is_read_only {
        attributes.insert("windows_uia.value.is_read_only".into(), value.into());
    }
    attributes
}

#[test]
fn value_supported_with_explicit_non_password_writable_facts_is_admissible() {
    let facts = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
        WindowsUiaPatternSupport::Supported,
        Some("false"),
        Some("false"),
    )));

    assert_eq!(facts.is_password(), WindowsUiaBooleanCapabilityFact::False);
    assert_eq!(facts.is_read_only(), WindowsUiaBooleanCapabilityFact::False);
    assert!(
        facts.permits_set_value(),
        "SetValue admission requires exact Value support plus explicit non-password and writable facts"
    );
}

#[test]
fn password_true_or_unknown_fails_closed_even_when_value_pattern_is_supported() {
    for is_password in [None, Some("true"), Some("unknown"), Some("malformed")] {
        let facts = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
            WindowsUiaPatternSupport::Supported,
            is_password,
            Some("false"),
        )));
        assert!(
            !facts.permits_set_value(),
            "password state {is_password:?} must never authorize SetValue"
        );
    }
}

#[test]
fn readonly_true_or_unknown_fails_closed_even_when_value_pattern_is_supported() {
    for is_read_only in [None, Some("true"), Some("unknown"), Some("malformed")] {
        let facts = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
            WindowsUiaPatternSupport::Supported,
            Some("false"),
            is_read_only,
        )));
        assert!(
            !facts.permits_set_value(),
            "read-only state {is_read_only:?} must never authorize SetValue"
        );
    }
}

#[test]
fn unsupported_or_unknown_value_pattern_never_becomes_writable_from_safe_boolean_facts() {
    for support in [
        WindowsUiaPatternSupport::Unsupported,
        WindowsUiaPatternSupport::Unknown,
    ] {
        let facts = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
            support,
            Some("false"),
            Some("false"),
        )));
        assert!(!facts.permits_set_value());
    }
}

#[test]
fn capability_boolean_wire_values_are_strict_and_unknown_by_default() {
    let missing = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
        WindowsUiaPatternSupport::Supported,
        None,
        None,
    )));
    assert_eq!(
        missing.is_password(),
        WindowsUiaBooleanCapabilityFact::Unknown
    );
    assert_eq!(
        missing.is_read_only(),
        WindowsUiaBooleanCapabilityFact::Unknown
    );

    let malformed = WindowsUiaValueCapabilityFacts::from_node(&node(value_attributes(
        WindowsUiaPatternSupport::Supported,
        Some("False"),
        Some("0"),
    )));
    assert_eq!(
        malformed.is_password(),
        WindowsUiaBooleanCapabilityFact::Unknown
    );
    assert_eq!(
        malformed.is_read_only(),
        WindowsUiaBooleanCapabilityFact::Unknown
    );
}
