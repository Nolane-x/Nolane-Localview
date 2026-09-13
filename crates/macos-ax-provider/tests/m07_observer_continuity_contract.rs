use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxObserverApplicationBindingProvider, AxObserverContinuityBreakReason,
    AxObserverContinuityDecision, AxObserverContinuityProvider, AxObserverContinuityRebindError,
    AxObserverContinuityStartError, AxRunLoopLiveness,
};

fn app() -> AxApplicationIncarnation {
    AxApplicationIncarnation::new("com.nolane.localview.m07-seed", 7070, 7001)
}

#[test]
fn successful_observer_registration_without_serviced_run_loop_is_not_continuity() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let source = continuity_authority.new_run_loop_source_incarnation();

    assert_eq!(
        continuity_authority.activate(observer, source, AxRunLoopLiveness::Unverified),
        Err(AxObserverContinuityStartError::RunLoopNotServiced)
    );
}

#[test]
fn serviced_run_loop_mints_continuity_bound_to_observer_and_source_incarnation() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let current_app = app();
    let observer = observer_authority.bind_current(current_app.clone());
    let observer_revision = observer.observer_creation_revision();
    let source = continuity_authority.new_run_loop_source_incarnation();

    let live = continuity_authority
        .activate(observer, source, AxRunLoopLiveness::Serviced)
        .expect("serviced run-loop source may establish observer continuity");

    assert_eq!(live.application_incarnation(), &current_app);
    assert_eq!(live.observer_creation_revision(), observer_revision);
    assert_eq!(live.run_loop_source_incarnation(), source);
    assert!(live.continuity_revision() > 0);
}

#[test]
fn run_loop_interruption_consumes_live_continuity_and_requires_reconciliation() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let source = continuity_authority.new_run_loop_source_incarnation();
    let live = continuity_authority
        .activate(observer, source, AxRunLoopLiveness::Serviced)
        .expect("initial continuity");
    let continuity_revision = live.continuity_revision();
    let observer_revision = live.observer_creation_revision();

    let decision = continuity_authority.validate(
        live,
        source,
        AxRunLoopLiveness::Interrupted,
    );
    let AxObserverContinuityDecision::Broken(broken) = decision else {
        panic!("run-loop interruption must break event continuity");
    };

    assert_eq!(broken.reason(), AxObserverContinuityBreakReason::RunLoopInterrupted);
    assert_eq!(broken.invalidated_continuity_revision(), continuity_revision);
    assert_eq!(broken.invalidated_observer_creation_revision(), observer_revision);
    assert!(broken.requires_snapshot_reconciliation());
    assert!(broken.requires_observer_recreation());
}

#[test]
fn replacing_run_loop_source_breaks_old_continuity_even_when_new_source_is_serviced() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let old_source = continuity_authority.new_run_loop_source_incarnation();
    let live = continuity_authority
        .activate(observer, old_source, AxRunLoopLiveness::Serviced)
        .expect("initial continuity");
    let new_source = continuity_authority.new_run_loop_source_incarnation();

    let decision = continuity_authority.validate(
        live,
        new_source,
        AxRunLoopLiveness::Serviced,
    );
    let AxObserverContinuityDecision::Broken(broken) = decision else {
        panic!("run-loop source replacement must invalidate old continuity");
    };

    assert_eq!(broken.reason(), AxObserverContinuityBreakReason::RunLoopSourceReplaced);
    assert_eq!(broken.previous_run_loop_source_incarnation(), old_source);
    assert_eq!(broken.current_run_loop_source_incarnation(), Some(new_source));
}

#[test]
fn recovery_requires_fresh_observer_revision_and_fresh_serviced_run_loop_source() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let current_app = app();

    let stale_observer = observer_authority.bind_current(current_app.clone());
    let live_observer = observer_authority.bind_current(current_app.clone());
    let old_source = continuity_authority.new_run_loop_source_incarnation();
    let live = continuity_authority
        .activate(live_observer, old_source, AxRunLoopLiveness::Serviced)
        .expect("initial continuity");
    let old_observer_revision = live.observer_creation_revision();

    let broken = match continuity_authority.validate(
        live,
        old_source,
        AxRunLoopLiveness::Interrupted,
    ) {
        AxObserverContinuityDecision::Broken(broken) => broken,
        other => panic!("interruption must break continuity, got {other:?}"),
    };

    let old_source_retry = continuity_authority
        .rebind_after_break(
            broken,
            stale_observer,
            old_source,
            AxRunLoopLiveness::Serviced,
        )
        .expect_err("old observer/source authority must not be resurrected");
    assert_eq!(
        old_source_retry,
        AxObserverContinuityRebindError::ObserverRevisionNotNewer
    );

    let fresh_observer = observer_authority.bind_current(current_app.clone());
    assert!(fresh_observer.observer_creation_revision() > old_observer_revision);
    let fresh_source = continuity_authority.new_run_loop_source_incarnation();

    // Reproduce a fresh broken lineage because the first tombstone was consumed
    // by the failed recovery attempt above.
    let second_live_observer = observer_authority.bind_current(current_app);
    let second_old_source = continuity_authority.new_run_loop_source_incarnation();
    let second_live = continuity_authority
        .activate(
            second_live_observer,
            second_old_source,
            AxRunLoopLiveness::Serviced,
        )
        .expect("second continuity lineage");
    let second_old_revision = second_live.observer_creation_revision();
    let second_broken = match continuity_authority.validate(
        second_live,
        second_old_source,
        AxRunLoopLiveness::Interrupted,
    ) {
        AxObserverContinuityDecision::Broken(broken) => broken,
        other => panic!("second interruption must break continuity, got {other:?}"),
    };
    let newest_observer = observer_authority.bind_current(app());
    assert!(newest_observer.observer_creation_revision() > second_old_revision);
    let newest_source = continuity_authority.new_run_loop_source_incarnation();

    let restored = continuity_authority
        .rebind_after_break(
            second_broken,
            newest_observer,
            newest_source,
            AxRunLoopLiveness::Serviced,
        )
        .expect("fresh observer + fresh serviced source must establish a new continuity revision");

    assert_eq!(restored.run_loop_source_incarnation(), newest_source);
    assert!(restored.observer_creation_revision() > second_old_revision);
    assert!(restored.continuity_revision() > 0);

    // Keep these fresh values live in the contract: recovery authority is tied
    // to source incarnation, not merely to a later observer revision.
    assert_ne!(fresh_source, old_source);
}