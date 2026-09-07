use localview_resource_governor::{
    DegradationAction, LiveSurfaceIdentity, ResourceActivationError, ResourceBudget,
    ResourceWorkKind, RuntimeResourceGovernor, SurfaceVisibility,
};

fn budget_with_one_hidden_surface() -> ResourceBudget {
    ResourceBudget {
        hidden_surfaces: 1,
        ..ResourceBudget::default()
    }
}

#[test]
fn pending_native_surface_holds_admission_before_platform_create() {
    let governor = RuntimeResourceGovernor::new(budget_with_one_hidden_surface());
    let pending = governor
        .reserve("session-a", "open-a", ResourceWorkKind::NativeSurface)
        .expect("first native surface admission");

    assert!(
        governor
            .reserve("session-b", "open-b", ResourceWorkKind::NativeSurface)
            .is_err(),
        "a pending native surface must reserve capacity before platform creation"
    );

    drop(pending);
    assert!(
        governor
            .reserve("session-b", "open-c", ResourceWorkKind::NativeSurface)
            .is_ok(),
        "dropping pending admission must reopen capacity"
    );
}

#[test]
fn hidden_live_surface_consumes_budget_until_matching_lease_drops() {
    let governor = RuntimeResourceGovernor::new(budget_with_one_hidden_surface());
    let pending = governor
        .reserve("session-a", "open-a", ResourceWorkKind::NativeSurface)
        .expect("first native surface admission");
    let identity = LiveSurfaceIdentity::new("preview_window", "preview-a", 1);
    let live = pending
        .activate_surface(identity, SurfaceVisibility::Hidden)
        .expect("native surface activation");

    assert!(
        governor
            .decision()
            .actions
            .contains(&DegradationAction::SuspendInactiveRenderSurfaces),
        "one hidden live surface at the declared limit must create hidden-surface pressure"
    );
    assert!(
        governor
            .reserve("session-b", "open-b", ResourceWorkKind::NativeSurface)
            .is_err(),
        "hidden live owner truth must keep the slot consumed"
    );

    drop(live);
    assert!(
        governor
            .reserve("session-b", "open-c", ResourceWorkKind::NativeSurface)
            .is_ok(),
        "dropping the exact live owner lease must reopen native-surface admission"
    );
}

#[test]
fn visible_surface_releases_hidden_slot_and_hide_transition_reclaims_it() {
    let governor = RuntimeResourceGovernor::new(budget_with_one_hidden_surface());
    let pending = governor
        .reserve("session-a", "open-a", ResourceWorkKind::NativeSurface)
        .expect("first native surface admission");
    let identity = LiveSurfaceIdentity::new("workspace_child", "workspace-a", 7);
    let live = pending
        .activate_surface(identity.clone(), SurfaceVisibility::Visible)
        .expect("visible native surface activation");

    assert!(
        !governor
            .decision()
            .actions
            .contains(&DegradationAction::SuspendInactiveRenderSurfaces),
        "visible native surfaces must not be counted as hidden"
    );
    let second = governor
        .reserve("session-b", "open-b", ResourceWorkKind::NativeSurface)
        .expect("visible surface should release the conservative pending hidden slot");
    drop(second);

    live.set_surface_visibility(identity.clone(), SurfaceVisibility::Hidden)
        .expect("exact owner may transition visibility");
    assert!(
        governor
            .decision()
            .actions
            .contains(&DegradationAction::SuspendInactiveRenderSurfaces),
        "hiding the exact live surface must restore hidden-surface pressure"
    );

    live.set_surface_visibility(identity, SurfaceVisibility::Visible)
        .expect("exact owner may become visible again");
    assert!(
        !governor
            .decision()
            .actions
            .contains(&DegradationAction::SuspendInactiveRenderSurfaces),
        "showing the surface must remove hidden-surface pressure without changing incarnation"
    );
}

#[test]
fn live_surface_rejects_visibility_update_for_foreign_identity() {
    let governor = RuntimeResourceGovernor::new(budget_with_one_hidden_surface());
    let pending = governor
        .reserve("session-a", "open-a", ResourceWorkKind::NativeSurface)
        .expect("native surface admission");
    let current = LiveSurfaceIdentity::new("preview_window", "preview-a", 2);
    let live = pending
        .activate_surface(current, SurfaceVisibility::Visible)
        .expect("native surface activation");
    let stale = LiveSurfaceIdentity::new("preview_window", "preview-a", 1);

    assert_eq!(
        live.set_surface_visibility(stale, SurfaceVisibility::Hidden),
        Err(ResourceActivationError::SurfaceIdentityMismatch),
        "stale incarnation must not mutate current live surface truth"
    );
    assert!(
        !governor
            .decision()
            .actions
            .contains(&DegradationAction::SuspendInactiveRenderSurfaces),
        "rejected stale update must not change current visibility"
    );
}

#[test]
fn releasing_session_does_not_forge_live_native_surface_exit() {
    let governor = RuntimeResourceGovernor::new(budget_with_one_hidden_surface());
    let pending = governor
        .reserve("session-a", "open-a", ResourceWorkKind::NativeSurface)
        .expect("native surface admission");
    let identity = LiveSurfaceIdentity::new("preview_window", "preview-a", 1);
    let live = pending
        .activate_surface(identity, SurfaceVisibility::Hidden)
        .expect("native surface activation");

    assert_eq!(
        governor.release_session("session-a"),
        0,
        "generic session cleanup may not forge disappearance of a desktop-owned live surface"
    );
    assert!(
        governor
            .reserve("session-b", "open-b", ResourceWorkKind::NativeSurface)
            .is_err(),
        "live native surface truth must survive pending-session cleanup"
    );

    drop(live);
    assert!(
        governor
            .reserve("session-b", "open-c", ResourceWorkKind::NativeSurface)
            .is_ok(),
        "exact live owner release must reopen capacity"
    );
}
