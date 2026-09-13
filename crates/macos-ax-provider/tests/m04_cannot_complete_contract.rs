use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxElementBindingProvider, AxElementIdentity,
    AxElementOperationDecision, AxElementReacquireDirective, AxElementRebindError,
};

const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

fn application() -> AxApplicationIncarnation {
    AxApplicationIncarnation::new("com.nolane.localview.m04-seed", 4343, 1)
}

fn identity() -> AxElementIdentity {
    AxElementIdentity::new(
        application(),
        "window:localview-m04-seed",
        "ax-identifier:localview-m04-target",
    )
}

#[test]
fn cannot_complete_consumes_binding_without_claiming_stale_element() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let old_sequence = binding.binding_sequence();

    let decision = provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE);
    let unresponsive = match decision {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("kAXErrorCannotComplete must become UnresponsiveTarget, got {other:?}"),
    };

    assert_eq!(unresponsive.invalidated_binding_sequence(), old_sequence);
    assert_eq!(
        unresponsive.reacquire_directive(),
        AxElementReacquireDirective::ReacquireCurrentIdentity
    );
}

#[test]
fn unresponsive_target_requires_same_identity_and_mints_new_binding() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let old_sequence = binding.binding_sequence();
    let unresponsive = match provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE) {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("expected unresponsive tombstone, got {other:?}"),
    };

    let rebound = provider
        .rebind_after_unresponsive_reacquire(unresponsive, identity())
        .expect("same semantic identity may establish a fresh post-timeout binding");
    assert!(rebound.binding_sequence() > old_sequence);
}

#[test]
fn unresponsive_reacquire_cannot_switch_semantic_identity() {
    let provider = AxElementBindingProvider::new();
    let binding = provider.bind_current(identity());
    let unresponsive = match provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE) {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("expected unresponsive tombstone, got {other:?}"),
    };

    let different = AxElementIdentity::new(
        application(),
        "window:localview-m04-seed",
        "ax-identifier:different-target",
    );
    let error = provider
        .rebind_after_unresponsive_reacquire(unresponsive, different)
        .expect_err("M04 must not move timed-out authority to a different semantic target");

    assert_eq!(error, AxElementRebindError::IdentityChanged);
}
