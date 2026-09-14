#![cfg(all(target_os = "linux", feature = "validation-state-injection"))]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiBindingLifecycle, AtspiEndpoint, AtspiReacquireError,
    LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider_with(provider: &str, target: &str) -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from(provider),
        TargetIncarnationRef::from(target),
    )
}

fn provider() -> LinuxAtspiProvider {
    provider_with(
        "provider:linux-atspi:l02:stable",
        "target:linux-atspi:l02:stable",
    )
}

#[test]
fn live_binding_cannot_be_used_as_recreate_authority() {
    let provider = provider();
    let old = provider.bind(
        AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/22"),
        "cut:l02:old-live",
    );

    assert_eq!(
        provider.reacquire_after_defunct(
            &old,
            old.endpoint().clone(),
            "cut:l02:replacement-too-early",
        ),
        Err(AtspiReacquireError::PreviousBindingNotDefunct)
    );
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::Live);
}

#[test]
fn recreate_requires_previous_binding_from_same_provider_and_target_incarnation() {
    let owner = provider();
    let old = owner.bind(
        AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/23"),
        "cut:l02:foreign-old",
    );
    assert_eq!(
        owner.authorize_from_state_set_for_validation(&old, StateSet::new(State::Defunct)),
        Err(AtspiActionEligibilityError::Defunct)
    );

    let wrong_provider = provider_with(
        "provider:linux-atspi:l02:other",
        "target:linux-atspi:l02:stable",
    );
    assert_eq!(
        wrong_provider.reacquire_after_defunct(
            &old,
            old.endpoint().clone(),
            "cut:l02:wrong-provider",
        ),
        Err(AtspiReacquireError::ProviderIncarnationMismatch)
    );

    let wrong_target = provider_with(
        "provider:linux-atspi:l02:stable",
        "target:linux-atspi:l02:other",
    );
    assert_eq!(
        wrong_target.reacquire_after_defunct(
            &old,
            old.endpoint().clone(),
            "cut:l02:wrong-target",
        ),
        Err(AtspiReacquireError::TargetIncarnationMismatch)
    );
}

#[test]
fn defunct_recreate_mints_fresh_authority_without_resurrecting_old_binding() {
    let provider = provider();
    let endpoint = AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/24");
    let old = provider.bind(endpoint.clone(), "cut:l02:old");
    let old_revision = old.binding_revision();

    assert!(provider
        .authorize_from_state_set_for_validation(&old, StateSet::empty())
        .is_ok());
    assert_eq!(
        provider.authorize_from_state_set_for_validation(
            &old,
            StateSet::new(State::Defunct),
        ),
        Err(AtspiActionEligibilityError::Defunct)
    );
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);

    let fresh = provider
        .reacquire_after_defunct(&old, endpoint, "cut:l02:fresh")
        .expect("typed DEFUNCT is the only authority for L02 recreation");

    assert!(fresh.binding_revision() > old_revision);
    assert_eq!(fresh.acquisition_cut_ref(), "cut:l02:fresh");
    assert_eq!(fresh.lifecycle(), AtspiBindingLifecycle::Live);
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);

    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, StateSet::empty()),
        Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
    );
    assert!(provider
        .authorize_from_state_set_for_validation(&fresh, StateSet::empty())
        .is_ok());
}
