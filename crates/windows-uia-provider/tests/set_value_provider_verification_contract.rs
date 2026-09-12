use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    SetValueMode, SetValuePayloadRef, WindowsUiaAttachment, WindowsUiaSetValueVerificationRequest,
    WindowsUiaWorker,
};
use uuid::Uuid;

fn element_ref(
    provider: &ProviderIncarnationRef,
    target: &TargetIncarnationRef,
) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows-uia".into(),
        provider_incarnation_ref: provider.clone(),
        target_incarnation_ref: target.clone(),
        opaque_provider_element_id: "runtime-id:42.7".into(),
        semantic_locator_hints: vec!["control_type:edit".into()],
        parent_surface_ref: None,
        acquisition_cut_ref: "cut:before-dispatch".into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-runtime-id-v1".into(),
    }
}

#[test]
fn provider_verification_request_keeps_expected_value_process_local_and_cut_separate() {
    let provider = ProviderIncarnationRef::from("provider:uia:test");
    let target = TargetIncarnationRef::from("target:uia:test");
    let element = element_ref(&provider, &target);
    let payload_ref = SetValuePayloadRef(Uuid::new_v4());
    let action_id = Uuid::new_v4();
    let sentinel = "LOCALVIEW_TASK7_FRESH_EQUALITY_SENTINEL";

    let request = WindowsUiaSetValueVerificationRequest::new(
        action_id,
        payload_ref,
        SetValueMode::ReplaceValue,
        provider.clone(),
        target.clone(),
        element.clone(),
        "cut:after-dispatch:fresh".into(),
        sentinel.as_bytes().to_vec(),
    )
    .expect("construct bounded fresh SetValue verification request");

    assert_eq!(request.action_id, action_id);
    assert_eq!(request.payload_ref, payload_ref);
    assert_eq!(request.mode, SetValueMode::ReplaceValue);
    assert_eq!(request.provider_incarnation_ref, provider);
    assert_eq!(request.target_incarnation_ref, target);
    assert_eq!(request.element_ref, element);
    assert_eq!(request.observation_cut_ref, "cut:after-dispatch:fresh");
    assert_eq!(request.expected_utf8_len(), sentinel.len());
    assert_ne!(
        request.element_ref.acquisition_cut_ref,
        request.observation_cut_ref
    );
    assert!(
        !format!("{request:?}").contains(sentinel),
        "provider verification Debug must never expose expected plaintext"
    );
}

#[allow(dead_code)]
fn compile_requires_worker_fresh_equality_boundary(
    worker: &WindowsUiaWorker,
    attachment: &WindowsUiaAttachment,
    request: WindowsUiaSetValueVerificationRequest,
) {
    let _ = worker.verify_set_value(attachment, request);
}
