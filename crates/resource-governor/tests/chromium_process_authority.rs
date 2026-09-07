use localview_resource_governor::{
    LiveResourceKind, ResourceActivationError, ResourceWorkKind, RuntimeResourceGovernor,
};

#[test]
fn chromium_activation_keeps_one_slot_consumed_until_live_lease_drops() {
    let governor = RuntimeResourceGovernor::default();
    let pending = governor
        .reserve("session-a", "request-a", ResourceWorkKind::Chromium)
        .expect("first Chromium admission");

    let live = pending
        .activate_live(LiveResourceKind::ChromiumProcess)
        .expect("spawn transition");

    assert!(
        governor
            .reserve("session-b", "request-b", ResourceWorkKind::Chromium)
            .is_err(),
        "pending -> live transition must not open a zero-count race window"
    );

    drop(live);

    assert!(
        governor
            .reserve("session-b", "request-c", ResourceWorkKind::Chromium)
            .is_ok(),
        "dropping the process-owner lease must reopen Chromium admission"
    );
}

#[test]
fn releasing_session_does_not_forge_live_chromium_exit() {
    let governor = RuntimeResourceGovernor::default();
    let pending = governor
        .reserve("session-a", "request-a", ResourceWorkKind::Chromium)
        .expect("first Chromium admission");
    let live = pending
        .activate_live(LiveResourceKind::ChromiumProcess)
        .expect("spawn transition");

    assert_eq!(
        governor.release_session("session-a"),
        0,
        "session cleanup may release pending work but not a live process-owner lease"
    );
    assert!(
        governor
            .reserve("session-b", "request-b", ResourceWorkKind::Chromium)
            .is_err(),
        "live process truth must survive pending-session cleanup"
    );

    drop(live);
    assert!(
        governor
            .reserve("session-b", "request-c", ResourceWorkKind::Chromium)
            .is_ok()
    );
}

#[test]
fn non_chromium_reservation_cannot_activate_a_chromium_process() {
    let governor = RuntimeResourceGovernor::default();
    let pending = governor
        .reserve(
            "session-a",
            "request-a",
            ResourceWorkKind::NativeVisualCapture,
        )
        .expect("native capture admission");

    let error = pending
        .activate_live(LiveResourceKind::ChromiumProcess)
        .expect_err("capture reservation cannot become a Chromium process");

    assert_eq!(error, ResourceActivationError::KindMismatch);
}
