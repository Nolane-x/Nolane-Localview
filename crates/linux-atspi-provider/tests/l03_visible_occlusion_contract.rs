#![cfg(all(target_os = "linux", feature = "validation-state-injection"))]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiEndpoint, AtspiPointerEligibilityError,
    AtspiPointerHitTest, LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider() -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from("provider:linux-atspi:l03:stable"),
        TargetIncarnationRef::from("target:linux-atspi:l03:stable"),
    )
}

fn binding(
    provider: &LinuxAtspiProvider,
    endpoint: AtspiEndpoint,
) -> localview_linux_atspi_provider::AtspiElementBinding {
    provider
        .bind_initial(endpoint, "cut:l03:initial")
        .expect("initial L03 binding")
}

fn visible_showing() -> StateSet {
    StateSet::new(State::Visible) | StateSet::new(State::Showing)
}

#[test]
fn visible_target_hit_by_other_accessible_is_typed_occluded() {
    let provider = provider();
    let target = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/30");
    let blocker = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/31");
    let binding = binding(&provider, target);

    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            visible_showing(),
            AtspiPointerHitTest::Other(blocker),
        ),
        Err(AtspiPointerEligibilityError::Occluded)
    );
}

#[test]
fn visible_state_without_authoritative_hit_test_fails_closed() {
    let provider = provider();
    let target = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/32");
    let binding = binding(&provider, target);

    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            visible_showing(),
            AtspiPointerHitTest::Unavailable,
        ),
        Err(AtspiPointerEligibilityError::HitTestUnavailable)
    );
}

#[test]
fn invisible_or_not_showing_target_cannot_mint_pointer_authority() {
    let provider = provider();
    let target = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/33");
    let binding = binding(&provider, target.clone());

    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            StateSet::new(State::Showing),
            AtspiPointerHitTest::Target(target.clone()),
        ),
        Err(AtspiPointerEligibilityError::NotVisible)
    );
    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            StateSet::new(State::Visible),
            AtspiPointerHitTest::Target(target),
        ),
        Err(AtspiPointerEligibilityError::NotShowing)
    );
}

#[test]
fn pointer_authority_requires_visible_showing_and_exact_target_hit() {
    let provider = provider();
    let target = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/34");
    let binding = binding(&provider, target.clone());

    let permit = provider
        .authorize_pointer_from_observation_for_validation(
            &binding,
            visible_showing(),
            AtspiPointerHitTest::Target(target),
        )
        .expect("fresh exact hit-test must permit pointer targeting");

    assert_eq!(permit.binding_revision(), binding.binding_revision());
}

#[test]
fn defunct_semantics_still_win_before_pointer_exposure() {
    let provider = provider();
    let target = AtspiEndpoint::new(":1.230", "/org/a11y/atspi/accessible/35");
    let binding = binding(&provider, target.clone());
    let mut states = visible_showing();
    states.insert(State::Defunct);

    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            states,
            AtspiPointerHitTest::Target(target),
        ),
        Err(AtspiPointerEligibilityError::Semantic(
            AtspiActionEligibilityError::Defunct
        ))
    );
    assert_eq!(
        provider.authorize_pointer_from_observation_for_validation(
            &binding,
            visible_showing(),
            AtspiPointerHitTest::Unavailable,
        ),
        Err(AtspiPointerEligibilityError::Semantic(
            AtspiActionEligibilityError::AlreadyInvalidDefunct
        ))
    );
}
