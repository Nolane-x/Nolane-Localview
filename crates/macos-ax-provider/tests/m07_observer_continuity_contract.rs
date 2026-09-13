use std::{ffi::c_void, ptr};

use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxObserverApplicationBindingProvider, AxObserverContinuityBreakReason,
    AxObserverContinuityProvider, AxObserverContinuityRebindError, AxRunLoopCallbackTracker,
};

fn app() -> AxApplicationIncarnation {
    AxApplicationIncarnation::new("com.nolane.localview.m07-seed", 7070, 7001)
}

fn deliver_callback(tracker: &AxRunLoopCallbackTracker) {
    let callback = tracker.callback_function();
    unsafe {
        callback(
            ptr::null::<c_void>(),
            ptr::null::<c_void>(),
            ptr::null::<c_void>(),
            tracker.callback_refcon(),
        );
    }
}

#[test]
fn observer_registration_without_delivered_callback_cannot_mint_continuity() {
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
        "observer registration/source attachment alone must not mint serviced continuity evidence"
    );
}

#[test]
fn delivered_callback_mints_opaque_evidence_bound_to_observer_and_source() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let current_app = app();
    let observer = observer_authority.bind_current(current_app.clone());
    let observer_revision = observer.observer_creation_revision();
    let source = continuity_authority.new_run_loop_source_incarnation();
    let tracker = continuity_authority.new_callback_tracker(&observer, source);

    deliver_callback(&tracker);
    let evidence = continuity_authority
        .serviced_callback_evidence(&tracker)
        .expect("delivered callback must mint provider-owned serviced evidence");
    let live = continuity_authority
        .activate(observer, source, evidence)
        .expect("callback-backed service evidence may establish observer continuity");

    assert_eq!(tracker.delivered_callback_count(), 1);
    assert_eq!(live.application_incarnation(), &current_app);
    assert_eq!(live.observer_creation_revision(), observer_revision);
    assert_eq!(live.run_loop_source_incarnation(), source);
    assert_eq!(live.last_delivered_callback_count(), 1);
    assert!(live.continuity_revision() > 0);
}

#[test]
fn run_loop_interruption_consumes_live_continuity_and_requires_reconciliation() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let source = continuity_authority.new_run_loop_source_incarnation();
    let tracker = continuity_authority.new_callback_tracker(&observer, source);
    deliver_callback(&tracker);
    let evidence = continuity_authority
        .serviced_callback_evidence(&tracker)
        .expect("initial callback evidence");
    let live = continuity_authority
        .activate(observer, source, evidence)
        .expect("initial continuity");
    let continuity_revision = live.continuity_revision();
    let observer_revision = live.observer_creation_revision();

    let broken = continuity_authority.break_continuity(
        live,
        source,
        AxObserverContinuityBreakReason::RunLoopInterrupted,
    );

    assert_eq!(broken.reason(), AxObserverContinuityBreakReason::RunLoopInterrupted);
    assert_eq!(broken.invalidated_continuity_revision(), continuity_revision);
    assert_eq!(broken.invalidated_observer_creation_revision(), observer_revision);
    assert!(broken.requires_snapshot_reconciliation());
    assert!(broken.requires_observer_recreation());
}

#[test]
fn replacing_run_loop_source_breaks_old_continuity_even_without_positive_liveness_claim() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let observer = observer_authority.bind_current(app());
    let old_source = continuity_authority.new_run_loop_source_incarnation();
    let tracker = continuity_authority.new_callback_tracker(&observer, old_source);
    deliver_callback(&tracker);
    let evidence = continuity_authority
        .serviced_callback_evidence(&tracker)
        .expect("initial callback evidence");
    let live = continuity_authority
        .activate(observer, old_source, evidence)
        .expect("initial continuity");
    let new_source = continuity_authority.new_run_loop_source_incarnation();

    let broken = continuity_authority.break_continuity(
        live,
        new_source,
        AxObserverContinuityBreakReason::RunLoopLivenessUnverified,
    );

    assert_eq!(broken.reason(), AxObserverContinuityBreakReason::RunLoopSourceReplaced);
    assert_eq!(broken.previous_run_loop_source_incarnation(), old_source);
    assert_eq!(broken.current_run_loop_source_incarnation(), Some(new_source));
}

#[test]
fn recovery_requires_fresh_observer_revision_fresh_source_and_callback_evidence() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let current_app = app();

    let old_observer = observer_authority.bind_current(current_app.clone());
    let old_source = continuity_authority.new_run_loop_source_incarnation();
    let old_tracker = continuity_authority.new_callback_tracker(&old_observer, old_source);
    deliver_callback(&old_tracker);
    let old_evidence = continuity_authority
        .serviced_callback_evidence(&old_tracker)
        .expect("old callback evidence");
    let live = continuity_authority
        .activate(old_observer, old_source, old_evidence)
        .expect("initial continuity");
    let old_observer_revision = live.observer_creation_revision();

    let broken = continuity_authority.break_continuity(
        live,
        old_source,
        AxObserverContinuityBreakReason::RunLoopInterrupted,
    );

    let stale_observer = observer_authority.bind_current(current_app.clone());
    let stale_source = continuity_authority.new_run_loop_source_incarnation();
    let stale_tracker = continuity_authority.new_callback_tracker(&stale_observer, stale_source);
    assert!(
        continuity_authority
            .serviced_callback_evidence(&stale_tracker)
            .is_none(),
        "fresh observer/source without an actual delivered callback must not restore continuity"
    );

    deliver_callback(&stale_tracker);
    let stale_evidence = continuity_authority
        .serviced_callback_evidence(&stale_tracker)
        .expect("fresh callback evidence");
    let restored = continuity_authority
        .rebind_after_break(broken, stale_observer, stale_source, stale_evidence)
        .expect("fresh observer + source + delivered callback must restore continuity");

    assert!(restored.observer_creation_revision() > old_observer_revision);
    assert_eq!(restored.run_loop_source_incarnation(), stale_source);
    assert_eq!(restored.last_delivered_callback_count(), 1);
}

#[test]
fn service_evidence_from_the_wrong_observer_cannot_restore_a_broken_lineage() {
    let observer_authority = AxObserverApplicationBindingProvider::new();
    let continuity_authority = AxObserverContinuityProvider::new();
    let current_app = app();

    let old_observer = observer_authority.bind_current(current_app.clone());
    let old_source = continuity_authority.new_run_loop_source_incarnation();
    let old_tracker = continuity_authority.new_callback_tracker(&old_observer, old_source);
    deliver_callback(&old_tracker);
    let old_evidence = continuity_authority
        .serviced_callback_evidence(&old_tracker)
        .expect("old callback evidence");
    let live = continuity_authority
        .activate(old_observer, old_source, old_evidence)
        .expect("initial continuity");
    let broken = continuity_authority.break_continuity(
        live,
        old_source,
        AxObserverContinuityBreakReason::RunLoopInterrupted,
    );

    let fresh_observer = observer_authority.bind_current(current_app.clone());
    let fresh_source = continuity_authority.new_run_loop_source_incarnation();
    let wrong_observer = observer_authority.bind_current(current_app);
    let wrong_tracker = continuity_authority.new_callback_tracker(&wrong_observer, fresh_source);
    deliver_callback(&wrong_tracker);
    let wrong_evidence = continuity_authority
        .serviced_callback_evidence(&wrong_tracker)
        .expect("wrong observer callback evidence");

    assert_eq!(
        continuity_authority
            .rebind_after_break(broken, fresh_observer, fresh_source, wrong_evidence)
            .expect_err("callback evidence must be bound to the exact fresh observer revision"),
        AxObserverContinuityRebindError::ServiceEvidenceMismatch
    );
}
