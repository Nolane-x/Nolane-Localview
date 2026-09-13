use localview_macos_ax_provider::{
    AxElementBindingProvider, AxElementIdentity, AxElementOperationDecision,
    AxElementReacquireDirective, AxElementRebindError,
};

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;

fn identity() -> AxElementIdentity {
    AxElementIdentity::new(
        4242,
        "window:localview-m03-seed",
        "ax-identifier:localview-m03-target",
    )
}

#[test]
fn invalid_ui_element_consumes_current_binding_and_requires_reacquire() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let old_sequence = binding.binding_sequence();

    let decision = provider.observe_operation(binding, AX_ERROR_INVALID_UI_ELEMENT);
    let stale = match decision {
        AxElementOperationDecision::StaleTarget(stale) => stale,
        other => panic!("invalid AX element must become StaleTarget, got {other:?}"),
    };

    assert_eq!(stale.invalidated_binding_sequence(), old_sequence);
    assert_eq!(
        stale.reacquire_directive(),
        AxElementReacquireDirective::ReacquireCurrentIdentity
    );

    let rebound = provider
        .rebind_after_reacquire(stale, identity())
        .expect("same semantic identity may establish a fresh binding");
    assert!(rebound.binding_sequence() > old_sequence);
}

#[test]
fn successful_ax_operation_preserves_the_current_binding_revision() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let sequence = binding.binding_sequence();

    let decision = provider.observe_operation(binding, AX_ERROR_SUCCESS);
    let current = match decision {
        AxElementOperationDecision::Current(current) => current,
        other => panic!("successful AX operation must preserve the binding, got {other:?}"),
    };

    assert_eq!(current.binding_sequence(), sequence);
}

#[test]
fn reacquire_cannot_silently_switch_to_a_different_semantic_identity() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let decision = provider.observe_operation(binding, AX_ERROR_INVALID_UI_ELEMENT);
    let stale = match decision {
        AxElementOperationDecision::StaleTarget(stale) => stale,
        other => panic!("expected stale binding, got {other:?}"),
    };

    let different = AxElementIdentity::new(
        4242,
        "window:localview-m03-seed",
        "ax-identifier:different-target",
    );
    let error = provider
        .rebind_after_reacquire(stale, different)
        .expect_err("M03 must not resurrect stale authority onto a different semantic target");

    assert_eq!(error, AxElementRebindError::IdentityChanged);
}
