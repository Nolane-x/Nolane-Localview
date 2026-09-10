use localview_live_bridge::{SetValueMode, SetValuePayloadRef};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaSetValueEquality, WindowsUiaSetValueVerificationReceipt,
};
use uuid::Uuid;

fn element_ref() -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:set-value-equality"),
        target_incarnation_ref: TargetIncarnationRef::from("target:uia:set-value-equality"),
        opaque_provider_element_id: "uia-runtime:[77,1]".into(),
        semantic_locator_hints: vec!["automation_id=set-value-equality".into()],
        parent_surface_ref: Some("window:set-value-equality".into()),
        acquisition_cut_ref: "cut:pre:set-value-equality".into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

#[test]
fn set_value_verification_receipt_is_typed_metadata_only_evidence() {
    let element_ref = element_ref();
    let payload_ref = SetValuePayloadRef(Uuid::from_u128(0x7001));
    let receipt = WindowsUiaSetValueVerificationReceipt {
        action_id: Uuid::from_u128(0x7002),
        payload_ref,
        mode: SetValueMode::ReplaceValue,
        provider_incarnation_ref: element_ref.provider_incarnation_ref.clone(),
        target_incarnation_ref: element_ref.target_incarnation_ref.clone(),
        element_ref: element_ref.clone(),
        observation_cut_ref: "cut:post:set-value-equality".into(),
        equality: WindowsUiaSetValueEquality::Mismatch,
    };

    assert_eq!(receipt.payload_ref, payload_ref);
    assert_eq!(receipt.mode, SetValueMode::ReplaceValue);
    assert_eq!(receipt.element_ref, element_ref);
    assert_eq!(receipt.equality, WindowsUiaSetValueEquality::Mismatch);
    assert_ne!(
        WindowsUiaSetValueEquality::Match,
        WindowsUiaSetValueEquality::Mismatch
    );
    assert_ne!(
        WindowsUiaSetValueEquality::Mismatch,
        WindowsUiaSetValueEquality::Unknown
    );

    // The approved receipt schema has no expected/current plaintext field. Keep
    // Debug metadata-only as a defense against future accidental value leakage.
    let debug = format!("{receipt:?}");
    assert!(!debug.contains("expected_value"));
    assert!(!debug.contains("current_value"));
    assert!(!debug.contains("sentinel-set-value-plaintext"));
}
