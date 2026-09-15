#![cfg(all(target_os = "linux", feature = "validation-state-injection"))]

use atspi::{State, StateSet};
use localview_linux_atspi_provider::{
    AtspiActionEligibilityError, AtspiEndpoint, AtspiEventAssurance,
    AtspiEventReliabilityProfile, AtspiObservationOrigin, AtspiSemanticDimension,
    LinuxAtspiProvider,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

fn provider() -> LinuxAtspiProvider {
    LinuxAtspiProvider::new_for_validation(
        ProviderIncarnationRef::from("provider:linux-atspi:l04:stable"),
        TargetIncarnationRef::from("target:linux-atspi:l04:stable"),
    )
}

fn states_with(state: State) -> StateSet {
    StateSet::new(State::Visible) | StateSet::new(State::Showing) | StateSet::new(state)
}

fn reliability() -> AtspiEventReliabilityProfile {
    AtspiEventReliabilityProfile::linux_toolkit_default([AtspiSemanticDimension::StateSet])
}

#[test]
fn toolkit_sensitive_event_profile_requires_direct_reconciliation() {
    let profile = reliability();

    assert_eq!(profile.assurance(), AtspiEventAssurance::Incomplete);
    assert!(profile.requires_direct_reconciliation_for(AtspiSemanticDimension::StateSet));
}

#[test]
fn missed_event_cache_cannot_mint_action_authority() {
    let provider = provider();
    let binding = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/40"),
            "cut:l04:initial",
        )
        .expect("initial L04 binding");
    let cached = provider
        .state_observation_from_event_for_validation(
            &binding,
            "cut:l04:event-cache",
            states_with(State::Enabled),
        )
        .expect("event-derived cache observation");

    assert_eq!(cached.origin(), AtspiObservationOrigin::EventCache);
    assert_eq!(
        provider.authorize_action_from_observation_for_validation(
            &binding,
            &reliability(),
            &cached,
        ),
        Err(AtspiActionEligibilityError::ReconciliationRequired)
    );
}

#[test]
fn direct_reconciliation_mints_fresh_revision_without_mutating_old_cache() {
    let provider = provider();
    let binding = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/41"),
            "cut:l04:initial",
        )
        .expect("initial L04 binding");
    let cached = provider
        .state_observation_from_event_for_validation(
            &binding,
            "cut:l04:event-cache",
            states_with(State::Enabled),
        )
        .expect("event-derived cache observation");
    let old_revision = cached.observation_revision();
    let old_cut = cached.snapshot_cut_ref().to_owned();
    let old_states = cached.states();

    let reconciled = provider
        .reconcile_state_from_state_set_for_validation(
            &binding,
            states_with(State::Checked),
        )
        .expect("direct reconciliation observation");

    assert_eq!(reconciled.origin(), AtspiObservationOrigin::DirectReconciliation);
    assert!(reconciled.observation_revision() > old_revision);
    assert_ne!(reconciled.snapshot_cut_ref(), old_cut);
    assert!(reconciled.states().contains(State::Checked));
    assert!(!reconciled.states().contains(State::Enabled));

    assert_eq!(cached.observation_revision(), old_revision);
    assert_eq!(cached.snapshot_cut_ref(), old_cut);
    assert_eq!(cached.states(), old_states);
    assert_eq!(reliability().assurance(), AtspiEventAssurance::Incomplete);
}

#[test]
fn fresh_direct_reconciliation_can_mint_authority_bound_to_new_cut() {
    let provider = provider();
    let binding = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/42"),
            "cut:l04:initial",
        )
        .expect("initial L04 binding");
    let reconciled = provider
        .reconcile_state_from_state_set_for_validation(
            &binding,
            states_with(State::Checked),
        )
        .expect("direct reconciliation observation");

    let permit = provider
        .authorize_action_from_observation_for_validation(
            &binding,
            &reliability(),
            &reconciled,
        )
        .expect("fresh direct reconciliation must satisfy action freshness");

    assert_eq!(permit.binding_revision(), binding.binding_revision());
    assert_eq!(
        permit.observation_revision(),
        Some(reconciled.observation_revision())
    );
    assert_eq!(
        permit.observation_cut_ref(),
        Some(reconciled.snapshot_cut_ref())
    );
}

#[test]
fn reconciled_observation_cannot_cross_binding_or_mix_cuts() {
    let provider = provider();
    let first = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/43"),
            "cut:l04:first",
        )
        .expect("first L04 binding");
    let second = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/44"),
            "cut:l04:second",
        )
        .expect("second L04 binding");
    let reconciled = provider
        .reconcile_state_from_state_set_for_validation(
            &first,
            states_with(State::Checked),
        )
        .expect("first direct reconciliation observation");

    assert_eq!(
        provider.authorize_action_from_observation_for_validation(
            &second,
            &reliability(),
            &reconciled,
        ),
        Err(AtspiActionEligibilityError::ObservationBindingMismatch)
    );
}

#[tokio::test]
async fn shipping_reconciliation_requires_live_atspi_observation() {
    let provider = provider();
    let binding = provider
        .bind_initial(
            AtspiEndpoint::new(":1.240", "/org/a11y/atspi/accessible/45"),
            "cut:l04:shipping",
        )
        .expect("shipping L04 binding");

    assert_eq!(
        provider.reconcile_action_state(&binding).await,
        Err(AtspiActionEligibilityError::ObservationUnavailable)
    );
}
