#![cfg(all(target_os = "linux", feature = "validation-state-injection"))]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiAccessibilityBusLifecycle, AtspiActionEligibilityError, AtspiBindError, AtspiEndpoint,
    AtspiProviderConnectionError, LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider() -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from("provider:linux-atspi:l05:stable"),
        TargetIncarnationRef::from("target:linux-atspi:l05:stable"),
    )
}

fn live_states() -> StateSet {
    StateSet::new(State::Visible) | StateSet::new(State::Showing) | StateSet::new(State::Enabled)
}

#[test]
fn accessibility_bus_disconnect_immediately_fences_old_binding_authority() {
    let mut provider = provider();
    let binding = provider
        .bind_initial(
            AtspiEndpoint::new(":1.250", "/org/a11y/atspi/accessible/50"),
            "cut:l05:old",
        )
        .expect("initial L05 binding");

    provider.mark_accessibility_bus_disconnected();

    assert_eq!(
        provider.accessibility_bus_lifecycle(),
        AtspiAccessibilityBusLifecycle::Disconnected
    );
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&binding, live_states()),
        Err(AtspiActionEligibilityError::AccessibilityBusDisconnected)
    );
}

#[test]
fn disconnected_bus_cannot_create_unobserved_initial_binding() {
    let mut provider = provider();
    provider.mark_accessibility_bus_disconnected();

    assert_eq!(
        provider.bind_initial(
            AtspiEndpoint::new(":1.250", "/org/a11y/atspi/accessible/50-new"),
            "cut:l05:disconnected",
        ),
        Err(AtspiBindError::AccessibilityBusDisconnected)
    );
}

#[test]
fn successful_bus_reconnect_changes_incarnation_and_old_binding_cannot_revive() {
    let mut provider = provider();
    let endpoint = AtspiEndpoint::new(":1.250", "/org/a11y/atspi/accessible/51");
    let old = provider
        .bind_initial(endpoint.clone(), "cut:l05:old")
        .expect("initial L05 binding");
    let old_bus = *old.accessibility_bus_incarnation_ref();

    provider.mark_accessibility_bus_disconnected();
    let fresh_bus = provider
        .reconnect_accessibility_bus_for_validation()
        .expect("validation reconnect models a successful fresh accessibility bus");

    assert_ne!(fresh_bus, old_bus);
    assert_eq!(
        provider.accessibility_bus_lifecycle(),
        AtspiAccessibilityBusLifecycle::Connected
    );
    assert_eq!(provider.accessibility_bus_incarnation_ref(), fresh_bus);
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, live_states()),
        Err(AtspiActionEligibilityError::AccessibilityBusIncarnationMismatch)
    );
    assert_eq!(
        provider.bind_initial(endpoint, "cut:l05:illegal-aba"),
        Err(AtspiBindError::EndpointAlreadyBound)
    );
}

#[test]
fn explicit_reacquire_after_bus_reconnect_mints_fresh_binding_on_new_bus() {
    let mut provider = provider();
    let endpoint = AtspiEndpoint::new(":1.250", "/org/a11y/atspi/accessible/52");
    let old = provider
        .bind_initial(endpoint.clone(), "cut:l05:old")
        .expect("initial L05 binding");
    let old_revision = old.binding_revision();
    let old_bus = *old.accessibility_bus_incarnation_ref();

    provider.mark_accessibility_bus_disconnected();
    let fresh_bus = provider
        .reconnect_accessibility_bus_for_validation()
        .expect("fresh validation bus");
    let fresh = provider
        .reacquire_after_bus_reconnect(&old, endpoint, "cut:l05:fresh")
        .expect("explicit L05 reacquire must be required after bus replacement");

    assert_ne!(fresh_bus, old_bus);
    assert!(fresh.binding_revision() > old_revision);
    assert_eq!(*fresh.accessibility_bus_incarnation_ref(), fresh_bus);
    provider
        .authorize_from_state_set_for_validation(&fresh, live_states())
        .expect("fresh binding on the current bus can mint authority");
    assert_eq!(
        provider.authorize_from_state_set_for_validation(&old, live_states()),
        Err(AtspiActionEligibilityError::AccessibilityBusIncarnationMismatch)
    );
}

#[test]
fn observations_and_permits_are_bound_to_accessibility_bus_incarnation() {
    let mut provider = provider();
    let endpoint = AtspiEndpoint::new(":1.250", "/org/a11y/atspi/accessible/53");
    let old = provider
        .bind_initial(endpoint.clone(), "cut:l05:old")
        .expect("initial L05 binding");
    let old_observation = provider
        .reconcile_state_from_state_set_for_validation(&old, live_states())
        .expect("old-bus direct observation");
    let old_permit = provider
        .authorize_from_state_set_for_validation(&old, live_states())
        .expect("old-bus permit before disconnect");

    assert_eq!(
        old_observation.accessibility_bus_incarnation_ref(),
        old.accessibility_bus_incarnation_ref()
    );
    assert_eq!(
        old_permit.accessibility_bus_incarnation_ref(),
        old.accessibility_bus_incarnation_ref()
    );

    provider.mark_accessibility_bus_disconnected();
    provider
        .reconnect_accessibility_bus_for_validation()
        .expect("fresh validation bus");
    let fresh = provider
        .reacquire_after_bus_reconnect(&old, endpoint, "cut:l05:fresh")
        .expect("fresh L05 binding");
    let fresh_observation = provider
        .reconcile_state_from_state_set_for_validation(&fresh, live_states())
        .expect("fresh-bus observation");
    let fresh_permit = provider
        .authorize_from_state_set_for_validation(&fresh, live_states())
        .expect("fresh-bus permit");

    assert_ne!(
        old_observation.accessibility_bus_incarnation_ref(),
        fresh_observation.accessibility_bus_incarnation_ref()
    );
    assert_ne!(
        old_permit.accessibility_bus_incarnation_ref(),
        fresh_permit.accessibility_bus_incarnation_ref()
    );
}

#[tokio::test]
async fn reconnect_failure_stays_disconnected_and_does_not_advance_bus_incarnation() {
    let mut provider = provider();
    let old_bus = provider.accessibility_bus_incarnation_ref();
    provider.mark_accessibility_bus_disconnected();

    assert_eq!(
        provider.reconnect_accessibility_bus().await,
        Err(AtspiProviderConnectionError::AccessibilityBusUnavailable)
    );
    assert_eq!(
        provider.accessibility_bus_lifecycle(),
        AtspiAccessibilityBusLifecycle::Disconnected
    );
    assert_eq!(provider.accessibility_bus_incarnation_ref(), old_bus);
}

#[test]
fn reconnect_without_disconnect_is_rejected_instead_of_rotating_authority() {
    let mut provider = provider();
    let old_bus = provider.accessibility_bus_incarnation_ref();

    assert_eq!(
        provider.reconnect_accessibility_bus_for_validation(),
        Err(AtspiProviderConnectionError::AccessibilityBusStillConnected)
    );
    assert_eq!(provider.accessibility_bus_incarnation_ref(), old_bus);
    assert_eq!(
        provider.accessibility_bus_lifecycle(),
        AtspiAccessibilityBusLifecycle::Connected
    );
}
