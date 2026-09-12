use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaItemLookupProperty, WindowsUiaVirtualizedItemQueryRequest,
    WindowsUiaVirtualizedItemRealizeRequest, WindowsUiaVirtualizedItemRequestError,
};

fn element(cut: &str, realization: ProviderElementRealization) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("provider:w03"),
        target_incarnation_ref: TargetIncarnationRef::from("target:w03"),
        opaque_provider_element_id: "uia-runtime:[42,7]".into(),
        semantic_locator_hints: vec!["automation_id=virtual-list".into()],
        parent_surface_ref: Some("window:w03".into()),
        acquisition_cut_ref: cut.into(),
        realization,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

#[test]
fn item_container_query_requires_realized_exact_cut_and_nonempty_lookup_value() {
    let request = WindowsUiaVirtualizedItemQueryRequest::new(
        "cut:w03:1",
        element("cut:w03:1", ProviderElementRealization::RealizedCurrent),
        WindowsUiaItemLookupProperty::Name,
        "LocalView Virtual Item 255",
    )
    .expect("exact realized container should be queryable");
    assert_eq!(request.snapshot_cut_ref(), "cut:w03:1");
    assert_eq!(request.property(), WindowsUiaItemLookupProperty::Name);
    assert_eq!(request.value(), "LocalView Virtual Item 255");

    assert_eq!(
        WindowsUiaVirtualizedItemQueryRequest::new(
            "cut:w03:1",
            element("cut:w03:1", ProviderElementRealization::RealizedCurrent),
            WindowsUiaItemLookupProperty::Name,
            "   ",
        ),
        Err(WindowsUiaVirtualizedItemRequestError::EmptyLookupValue),
    );
    assert_eq!(
        WindowsUiaVirtualizedItemQueryRequest::new(
            "cut:w03:2",
            element("cut:w03:1", ProviderElementRealization::RealizedCurrent),
            WindowsUiaItemLookupProperty::AutomationId,
            "tail-item",
        ),
        Err(WindowsUiaVirtualizedItemRequestError::AcquisitionCutMismatch),
    );
    assert_eq!(
        WindowsUiaVirtualizedItemQueryRequest::new(
            "cut:w03:1",
            element("cut:w03:1", ProviderElementRealization::RealizationRequired,),
            WindowsUiaItemLookupProperty::Name,
            "LocalView Virtual Item 255",
        ),
        Err(WindowsUiaVirtualizedItemRequestError::ContainerNotRealized),
    );
}

#[test]
fn realization_request_accepts_only_exact_realization_required_placeholder() {
    let placeholder = element("cut:w03:1", ProviderElementRealization::RealizationRequired);
    let request = WindowsUiaVirtualizedItemRealizeRequest::new("cut:w03:1", placeholder.clone())
        .expect("virtual placeholder should be realizable");
    assert_eq!(request.snapshot_cut_ref(), "cut:w03:1");
    assert_eq!(request.placeholder_element_ref(), &placeholder);

    assert_eq!(
        WindowsUiaVirtualizedItemRealizeRequest::new(
            "cut:w03:2",
            element("cut:w03:1", ProviderElementRealization::RealizationRequired,),
        ),
        Err(WindowsUiaVirtualizedItemRequestError::AcquisitionCutMismatch),
    );
    assert_eq!(
        WindowsUiaVirtualizedItemRealizeRequest::new(
            "cut:w03:1",
            element("cut:w03:1", ProviderElementRealization::RealizedCurrent),
        ),
        Err(WindowsUiaVirtualizedItemRequestError::PlaceholderNotRealizationRequired),
    );
}
