use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaCoordinateSpace, WindowsUiaGeometryContractError, WindowsUiaGeometryRequest,
    WindowsUiaPhysicalRect,
};

fn element(cut: &str) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("provider:w10:geometry"),
        target_incarnation_ref: TargetIncarnationRef::from("target:w10:geometry"),
        opaque_provider_element_id: "uia-runtime:[10,10]".into(),
        semantic_locator_hints: vec!["automation_id=LocalViewW10GeometryTarget".into()],
        parent_surface_ref: Some("seed:w10:geometry".into()),
        acquisition_cut_ref: cut.into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

#[test]
fn physical_screen_rect_preserves_signed_coordinates_and_rejects_inversion() {
    let rect = WindowsUiaPhysicalRect::new(-240, 120, 760, 820).unwrap();
    assert_eq!(rect.left, -240);
    assert_eq!(rect.top, 120);
    assert_eq!(rect.right, 760);
    assert_eq!(rect.bottom, 820);
    assert_eq!(rect.width(), 1000);
    assert_eq!(rect.height(), 700);
    assert_eq!(
        WindowsUiaCoordinateSpace::PhysicalScreenPixels.as_wire_value(),
        "physical_screen_pixels"
    );

    assert_eq!(
        WindowsUiaPhysicalRect::new(5, 0, 4, 10),
        Err(WindowsUiaGeometryContractError::InvertedRectangle)
    );
    assert_eq!(
        WindowsUiaPhysicalRect::new(0, 5, 10, 4),
        Err(WindowsUiaGeometryContractError::InvertedRectangle)
    );
}

#[test]
fn geometry_request_is_bound_to_the_exact_snapshot_cut_and_element() {
    let exact = element("cut:w10:a");
    let request = WindowsUiaGeometryRequest::new("cut:w10:a", exact.clone()).unwrap();
    assert_eq!(request.snapshot_cut_ref(), "cut:w10:a");
    assert_eq!(request.element_ref(), &exact);

    assert_eq!(
        WindowsUiaGeometryRequest::new("", element("")),
        Err(WindowsUiaGeometryContractError::MissingSnapshotCut)
    );
    assert_eq!(
        WindowsUiaGeometryRequest::new("cut:w10:b", element("cut:w10:a")),
        Err(WindowsUiaGeometryContractError::ElementSnapshotCutMismatch)
    );
}
