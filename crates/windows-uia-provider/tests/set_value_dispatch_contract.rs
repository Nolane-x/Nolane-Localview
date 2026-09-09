use localview_native_provider::provider_element_ref_from_runtime_id;
use localview_protocol::{
    ProviderElementRealization, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    SetValueMode, SetValuePayloadRef, WindowsUiaDispatchContextRequirements,
    WindowsUiaSetValueDispatchRequest, WindowsUiaSetValueDispatchRequestError,
};
use uuid::Uuid;

fn request_with(
    mode: SetValueMode,
    secret: Vec<u8>,
) -> Result<WindowsUiaSetValueDispatchRequest, WindowsUiaSetValueDispatchRequestError> {
    let provider = ProviderIncarnationRef::from("provider:windows-uia:set-value-contract".to_string());
    let target = TargetIncarnationRef::from("target:windows-uia:set-value-contract".to_string());
    let element_ref = provider_element_ref_from_runtime_id(
        provider.clone(),
        target.clone(),
        &[42, 7],
        "cut:set-value-contract:1",
        ProviderElementRealization::Direct,
    );

    WindowsUiaSetValueDispatchRequest::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        17,
        "prepare:set-value-contract:17".into(),
        "cut:set-value-contract:1".into(),
        provider,
        target,
        element_ref,
        WindowsUiaDispatchContextRequirements {
            require_foreground_target: true,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        },
        SetValuePayloadRef(Uuid::new_v4()),
        mode,
        secret,
    )
}

#[test]
fn set_value_request_keeps_plaintext_out_of_debug_and_exposes_only_length() {
    let sentinel = b"localview-set-value-plaintext-sentinel".to_vec();
    let request = request_with(SetValueMode::ReplaceValue, sentinel.clone())
        .expect("valid bounded replacement payload");

    let debug = format!("{request:?}");
    assert!(!debug.contains("localview-set-value-plaintext-sentinel"));
    assert_eq!(request.secret_utf8_len(), sentinel.len());
    assert_eq!(request.mode, SetValueMode::ReplaceValue);
}

#[test]
fn replace_value_rejects_nul_and_payloads_larger_than_16_kib() {
    assert_eq!(
        request_with(SetValueMode::ReplaceValue, b"contains\0nul".to_vec()).unwrap_err(),
        WindowsUiaSetValueDispatchRequestError::PayloadContainsNul,
    );

    assert_eq!(
        request_with(SetValueMode::ReplaceValue, vec![b'x'; 16 * 1024 + 1]).unwrap_err(),
        WindowsUiaSetValueDispatchRequestError::PayloadTooLarge,
    );
}

#[test]
fn clear_value_accepts_only_an_empty_process_local_payload() {
    request_with(SetValueMode::ClearValue, Vec::new()).expect("empty clear payload is canonical");
    assert_eq!(
        request_with(SetValueMode::ClearValue, b"must-not-be-accepted".to_vec()).unwrap_err(),
        WindowsUiaSetValueDispatchRequestError::ClearValuePayloadMustBeEmpty,
    );
}

#[test]
fn request_rejects_nil_payload_identity_before_worker_dispatch() {
    let provider = ProviderIncarnationRef::from("provider:windows-uia:set-value-contract".to_string());
    let target = TargetIncarnationRef::from("target:windows-uia:set-value-contract".to_string());
    let element_ref = provider_element_ref_from_runtime_id(
        provider.clone(),
        target.clone(),
        &[42, 8],
        "cut:set-value-contract:2",
        ProviderElementRealization::Direct,
    );

    let error = WindowsUiaSetValueDispatchRequest::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        18,
        "prepare:set-value-contract:18".into(),
        "cut:set-value-contract:2".into(),
        provider,
        target,
        element_ref,
        WindowsUiaDispatchContextRequirements {
            require_foreground_target: false,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        },
        SetValuePayloadRef(Uuid::nil()),
        SetValueMode::ReplaceValue,
        b"bounded".to_vec(),
    )
    .unwrap_err();

    assert_eq!(error, WindowsUiaSetValueDispatchRequestError::InvalidPayloadRef);
}
