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
}
