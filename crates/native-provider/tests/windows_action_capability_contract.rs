use std::collections::{BTreeMap, BTreeSet};

use localview_native_provider::{
    NativeSemanticActionCapabilities, NativeSemanticNodeObservation, NativeSemanticPattern,
    NativeSemanticPatternSupport,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};

fn element(realization: ProviderElementRealization) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("provider:capability-contract"),
        target_incarnation_ref: TargetIncarnationRef::from("target:capability-contract"),
        opaque_provider_element_id: "uia-runtime:[42,7]".into(),
        semantic_locator_hints: vec![],
        parent_surface_ref: Some("window:capability-contract".into()),
        acquisition_cut_ref: "cut:capability-contract".into(),
        realization,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

#[test]
fn absent_pattern_evidence_is_unknown_not_unsupported() {
    let capabilities = NativeSemanticActionCapabilities::default();

    assert_eq!(
        capabilities.support_for(NativeSemanticPattern::Invoke),
        NativeSemanticPatternSupport::Unknown
    );
    assert_eq!(
        capabilities.support_for(NativeSemanticPattern::Value),
        NativeSemanticPatternSupport::Unknown
    );
}

#[test]
fn pattern_support_is_typed_and_round_trips_with_stable_wire_names() {
    let capabilities = NativeSemanticActionCapabilities {
        patterns: BTreeMap::from([
            (
                NativeSemanticPattern::Invoke,
                NativeSemanticPatternSupport::Supported,
            ),
            (
                NativeSemanticPattern::SelectionItem,
                NativeSemanticPatternSupport::Unsupported,
            ),
            (
                NativeSemanticPattern::Value,
                NativeSemanticPatternSupport::Unknown,
            ),
            (
                NativeSemanticPattern::VirtualizedItem,
                NativeSemanticPatternSupport::Supported,
            ),
        ]),
    };

    let encoded = serde_json::to_value(&capabilities).unwrap();
    assert_eq!(encoded["patterns"]["invoke"], "supported");
    assert_eq!(encoded["patterns"]["selection_item"], "unsupported");
    assert_eq!(encoded["patterns"]["value"], "unknown");
    assert_eq!(encoded["patterns"]["virtualized_item"], "supported");

    let decoded: NativeSemanticActionCapabilities = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, capabilities);
}

#[test]
fn typed_pattern_taxonomy_covers_the_first_reversible_windows_action_surface() {
    let patterns = BTreeSet::from([
        NativeSemanticPattern::Invoke,
        NativeSemanticPattern::SelectionItem,
        NativeSemanticPattern::Value,
        NativeSemanticPattern::Toggle,
        NativeSemanticPattern::ExpandCollapse,
        NativeSemanticPattern::ScrollItem,
        NativeSemanticPattern::VirtualizedItem,
    ]);

    assert_eq!(patterns.len(), 7);
}

#[test]
fn virtualization_capability_never_promotes_realization_state() {
    let node = NativeSemanticNodeObservation {
        element_ref: element(ProviderElementRealization::RealizationRequired),
        parent_index: None,
        depth: 0,
        role: Some("list item".into()),
        name: None,
        control_type: Some("uia_control_type:50007".into()),
        automation_id: None,
        class_name: None,
        is_enabled: None,
        is_offscreen: None,
        action_capabilities: NativeSemanticActionCapabilities {
            patterns: BTreeMap::from([(
                NativeSemanticPattern::VirtualizedItem,
                NativeSemanticPatternSupport::Supported,
            )]),
        },
        attributes: BTreeMap::new(),
    };

    assert_eq!(
        node.action_capabilities
            .support_for(NativeSemanticPattern::VirtualizedItem),
        NativeSemanticPatternSupport::Supported
    );
    assert_eq!(
        node.element_ref.realization,
        ProviderElementRealization::RealizationRequired,
        "pattern availability is capability evidence, not proof that the item is realized"
    );
}
