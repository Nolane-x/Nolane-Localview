#![cfg(target_os = "linux")]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiBindingLifecycle, AtspiEndpoint, LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider() -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from("provider:linux-atspi:test:1"),
        TargetIncarnationRef::from("target:linux-atspi:test:1"),
    )
}

fn provider_with(provider: &str, target: &str) -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from(provider),
        TargetIncarnationRef::from(target),
    )
}

#[test]
fn explicit_defunct_observation_terminally_blocks_action_eligibility() {
    let provider = provider();
    let binding = provider.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/7"),
        "cut:l01:1",
    );

    let live = provider
        .authorize_from_state_set_for_validation(&binding, StateSet::empty())
        .expect("non-defunct state may pass only the L01 liveness gate");
    assert_eq!(live.binding_revision(), binding.binding_revision());
    drop(live);

    let defunct = StateSet::new(State::Defunct);
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&binding, defunct),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(binding.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&binding, StateSet::empty()),
        Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
    );
}

#[test]
fn cloned_binding_shares_terminal_defunct_invalidation() {
    let provider = provider();
    let binding = provider.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/8"),
        "cut:l01:clone",
    );
    let stale_clone = binding.clone();

    assert_eq!(
        provider.authorize_from_state_set_for_validation(
            &binding,
            StateSet::new(State::Defunct),
        ),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&stale_clone, StateSet::empty()),
        Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
    );
}

#[test]
fn stale_and_visible_do_not_become_defunct() {
    let provider = provider();
    let binding = provider.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/9"),
        "cut:l01:states",
    );

    let states = StateSet::new(State::Stale | State::Visible);
    assert!(provider
        .authorize_from_state_set_for_validation(&binding, states)
        .is_ok());
    assert_eq!(binding.lifecycle(), AtspiBindingLifecycle::Live);
}

#[test]
fn provider_and_target_lineage_mismatch_are_typed() {
    let owner = provider_with(
        "provider:linux-atspi:test:owner",
        "target:linux-atspi:test:owner",
    );
    let binding = owner.bind(
        AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/10"),
        "cut:l01:lineage",
    );

    let wrong_provider = provider_with(
        "provider:linux-atspi:test:other",
        "target:linux-atspi:test:owner",
    );
    assert_eq!(
        wrong_provider.authorize_from_state_set_for_validation(&binding, StateSet::empty()),
        Err(AtspiActionEligibilityError::ProviderIncarnationMismatch)
    );

    let wrong_target = provider_with(
        "provider:linux-atspi:test:owner",
        "target:linux-atspi:test:other",
    );
    assert_eq!(
        wrong_target.authorize_from_state_set_for_validation(&binding, StateSet::empty()),
        Err(AtspiActionEligibilityError::TargetIncarnationMismatch)
    );
}

#[test]
fn same_endpoint_reuse_requires_a_new_binding_revision() {
    let provider = provider();
    let endpoint = AtspiEndpoint::new(":1.100", "/org/a11y/atspi/accessible/11");
    let old = provider.bind(endpoint.clone(), "cut:l01:old");
    let old_revision = old.binding_revision();

    assert_eq!(
        provider.authorize_from_state_set_for_validation(
            &old,
            StateSet::new(State::Defunct),
        ),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, StateSet::empty()),
        Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
    );

    let fresh = provider.reacquire(endpoint, "cut:l01:new");
    assert_ne!(fresh.binding_revision(), old_revision);
    assert!(provider
        .authorize_from_state_set_for_validation(&fresh, StateSet::empty())
        .is_ok());
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);
}
