use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxObserverApplicationBindingProvider, AxObserverContinuityProvider,
};

#[cfg(not(target_os = "macos"))]
use localview_macos_ax_provider::AxTrackedObserverCreateError;

fn app() -> AxApplicationIncarnation {
    AxApplicationIncarnation::new("com.nolane.localview.m07-seed", 7070, 7001)
}

#[test]
fn observer_registration_state_without_os_callback_cannot_mint_continuity() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let source = continuity_authority.new_run_loop_source_incarnation();
    let tracker = continuity_authority.new_callback_tracker(&observer, source);

    assert_eq!(tracker.delivered_callback_count(), 0);
    assert!(
        continuity_authority
            .serviced_callback_evidence(&tracker)
            .is_none(),
        "creating provider callback state alone must never mint serviced continuity evidence"
    );
}

#[test]
fn run_loop_source_incarnations_are_provider_owned_and_never_reused() {
    let continuity_authority = AxObserverContinuityProvider::new();
    let first = continuity_authority.new_run_loop_source_incarnation();
    let second = continuity_authority.new_run_loop_source_incarnation();

    assert!(second.sequence() > first.sequence());
    assert_ne!(first, second);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn native_tracked_observer_fails_closed_off_macos() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let source = continuity_authority.new_run_loop_source_incarnation();

    assert_eq!(
        continuity_authority
            .create_tracked_observer(&observer, source)
            .expect_err("non-macOS builds must not synthesize native AX observer authority"),
        AxTrackedObserverCreateError::PlatformUnsupported
    );
}
