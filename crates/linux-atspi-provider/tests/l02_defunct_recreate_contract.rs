#![cfg(all(target_os = "linux", feature = "validation-state-injection"))]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiBindError, AtspiBindingLifecycle, AtspiEndpoint,
    AtspiReacquireError, LinuxAtspiProvider,
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
fn duplicate_initial_bind_cannot_bypass_recreate_gate() {
    let provider = provider();
    let endpoint = AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/21");
    let first = provider
        .bind_initial(endpoint.clone(), "cut:l02:first")
        .expect("first acquisition for an endpoint is allowed");

    assert_eq!(
        provider
            .bind_initial(endpoint, "cut:l02:bypass")
            .expect_err("same endpoint cannot be minted again as an initial binding"),
        AtspiBindError::EndpointAlreadyBound
    );
    assert_eq!(first.lifecycle(), AtspiBindingLifecycle::Live);
}

#[test]
fn live_binding_cannot_be_used_as_recreate_authority() {
    let provider = provider();
    let old = provider
        .bind_initial(
            AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/22"),
            "cut:l02:old-live",
        )
        .expect("initial binding");

    assert_eq!(
        provider
            .reacquire_after_defunct(
                &old,
                old.endpoint().clone(),
                "cut:l02:replacement-too-early",
            )
            .expect_err("live binding must not authorize recreation"),
        AtspiReacquireError::PreviousBindingNotDefunct
    );
    assert_eq!(old.lifecycle(), AtspiBindingLifecycle::Live);
}

#[test]
fn recreate_requires_previous_binding_from_same_provider_and_target_incarnation() {
    let owner = provider();
    let old = owner
        .bind_initial(
            AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/23"),
            "cut:l02:foreign-old",
        )
        .expect("initial binding");
    assert_eq!(
        owner.authorize_from_state_set_for_validation(&old, StateSet::new(State::Defunct)),
        Err(AtspiActionEligibilityError::Defunct)
    );

    let wrong_provider = provider_with(
        "provider:linux-atspi:l02:other",
        "target:linux-atspi:l02:stable",
    );
    assert_eq!(
        wrong_provider
            .reacquire_after_defunct(
                &old,
                old.endpoint().clone(),
                "cut:l02:wrong-provider",
            )
            .expect_err("foreign provider cannot reuse the old binding"),
        AtspiReacquireError::ProviderIncarnationMismatch
    );

    let wrong_target = provider_with(
        "provider:linux-atspi:l02:stable",
        "target:linux-atspi:l02:other",
    );
    assert_eq!(
        wrong_target
            .reacquire_after_defunct(
                &old,
                old.endpoint().clone(),
                "cut:l02:wrong-target",
            )
            .expect_err("foreign target cannot reuse the old binding"),
        AtspiReacquireError::TargetIncarnationMismatch
    );
}

#[test]
fn defunct_recreate_mints_fresh_authority_without_resurrecting_or_replaying_old_binding() {
    let provider = provider();
    let endpoint = AtspiEndpoint::new(":1.220", "/org/a11y/atspi/accessible/24");
    let old = provider
        .bind_initial(endpoint.clone(), "cut:l02:old")
        .expect("initial binding");
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
        .reacquire_after_defunct(&old, endpoint.clone(), "cut:l02:fresh")
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

    assert_eq!(
        provider
            .reacquire_after_defunct(&old, endpoint, "cut:l02:replay")
            .expect_err("old DEFUNCT authority is single-use"),
        AtspiReacquireError::PreviousBindingSuperseded
    );
}
