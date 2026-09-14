use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxBoundedVisualEscalationAuthority, AxElementBindingProvider,
    AxElementIdentity, AxElementOperationDecision, AxVisualEscalationReason,
};

const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

fn identity() -> AxElementIdentity {
    AxElementIdentity::new(
        AxApplicationIncarnation::new("com.nolane.localview.m10-seed", 4545, 1),
        "window:localview-m10-seed",
        "ax-identifier:localview-m10-target",
    )
}

#[test]
fn timeout_creates_only_a_bounded_visual_escalation_request_not_capture_authority() {
    let binding_provider = AxElementBindingProvider::new();
    let binding = binding_provider.bind_current(identity());
    let invalidated_sequence = binding.binding_sequence();
    let unresponsive = match binding_provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE) {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("expected timeout/unresponsive tombstone, got {other:?}"),
    };

    let request = AxBoundedVisualEscalationAuthority::new()
        .request_after_unresponsive(unresponsive);

    assert_eq!(request.reason(), AxVisualEscalationReason::ProviderTimeout);
    assert_eq!(request.invalidated_binding_sequence(), invalidated_sequence);
    assert_eq!(request.max_visual_regions(), 1);
    assert!(!request.visual_capture_permitted());
    assert!(!request.input_fallback_permitted());
}
