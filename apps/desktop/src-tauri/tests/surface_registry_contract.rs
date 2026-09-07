#![forbid(unsafe_code)]

use localview_desktop::surface_registry::{
    DesktopSurfaceKind, DesktopSurfaceRegistry, DesktopSurfaceRegistryError,
    DesktopSurfaceVisibility,
};
use localview_protocol::SessionId;

fn session(value: &str) -> SessionId {
    value.parse().expect("valid session id")
}

#[test]
fn incarnation_is_monotonic_across_real_recreation() {
    let registry = DesktopSurfaceRegistry::default();
    let session_id = session("550e8400-e29b-41d4-a716-446655440000");

    let first = registry.next_identity(
        session_id,
        DesktopSurfaceKind::PreviewWindow,
        "preview-550e8400e29b41d4a7",
    );
    assert_eq!(first.incarnation, 1);
    registry
        .record_created(first.clone(), DesktopSurfaceVisibility::Visible)
        .expect("first physical surface creation");
    registry
        .record_closed(&first)
        .expect("first physical surface close");

    let second = registry.next_identity(
        session_id,
        DesktopSurfaceKind::PreviewWindow,
        "preview-550e8400e29b41d4a7",
    );
    assert_eq!(second.incarnation, 2);
    assert_eq!(second.session_id, first.session_id);
    assert_eq!(second.kind, first.kind);
    assert_eq!(second.label, first.label);
}

#[test]
fn duplicate_create_is_rejected_and_visibility_requires_exact_owner() {
    let registry = DesktopSurfaceRegistry::default();
    let session_id = session("550e8400-e29b-41d4-a716-446655440000");
    let identity = registry.next_identity(
        session_id,
        DesktopSurfaceKind::WorkspaceChild,
        "workspace-550e8400e29b41d4a7",
    );

    registry
        .record_created(identity.clone(), DesktopSurfaceVisibility::Visible)
        .expect("physical child creation");
    assert_eq!(
        registry.record_created(identity.clone(), DesktopSurfaceVisibility::Visible),
        Err(DesktopSurfaceRegistryError::AlreadyLive),
        "duplicate owner report may not create a second live surface"
    );

    let mut stale = identity.clone();
    stale.incarnation = identity.incarnation + 1;
    assert_eq!(
        registry.set_visibility(&stale, DesktopSurfaceVisibility::Hidden),
        Err(DesktopSurfaceRegistryError::IncarnationMismatch),
        "foreign incarnation may not mutate the current owner"
    );
    registry
        .set_visibility(&identity, DesktopSurfaceVisibility::Hidden)
        .expect("exact owner may become hidden");
    let current = registry.current(session_id, DesktopSurfaceKind::WorkspaceChild, &identity.label);
    assert_eq!(current.expect("current live owner").visibility, DesktopSurfaceVisibility::Hidden);
}

#[test]
fn stale_close_cannot_remove_a_newer_incarnation() {
    let registry = DesktopSurfaceRegistry::default();
    let session_id = session("550e8400-e29b-41d4-a716-446655440000");
    let label = "preview-550e8400e29b41d4a7";

    let first = registry.next_identity(session_id, DesktopSurfaceKind::PreviewWindow, label);
    registry
        .record_created(first.clone(), DesktopSurfaceVisibility::Visible)
        .expect("first creation");
    registry.record_closed(&first).expect("first close");

    let second = registry.next_identity(session_id, DesktopSurfaceKind::PreviewWindow, label);
    registry
        .record_created(second.clone(), DesktopSurfaceVisibility::Visible)
        .expect("second creation");

    assert_eq!(
        registry.record_closed(&first),
        Err(DesktopSurfaceRegistryError::IncarnationMismatch),
        "late close event from incarnation N may not remove incarnation N+1"
    );
    assert_eq!(
        registry
            .current(session_id, DesktopSurfaceKind::PreviewWindow, label)
            .expect("newer owner remains live")
            .identity,
        second
    );
}

#[test]
fn repeated_create_close_cycles_converge_to_zero_live_surfaces() {
    let registry = DesktopSurfaceRegistry::default();
    let session_id = session("550e8400-e29b-41d4-a716-446655440000");

    for _ in 0..32 {
        let identity = registry.next_identity(
            session_id,
            DesktopSurfaceKind::WorkspaceChild,
            "workspace-550e8400e29b41d4a7",
        );
        registry
            .record_created(identity.clone(), DesktopSurfaceVisibility::Hidden)
            .expect("create");
        registry.record_closed(&identity).expect("close");
    }

    assert_eq!(registry.live_count(), 0, "owner ledger must return to baseline");
}

#[test]
fn registry_only_exposes_target_surface_kinds_not_the_main_shell() {
    assert_eq!(DesktopSurfaceKind::PreviewWindow.as_runtime_kind(), "preview_window");
    assert_eq!(DesktopSurfaceKind::WorkspaceChild.as_runtime_kind(), "workspace_child");
    assert_eq!(DesktopSurfaceKind::from_runtime_kind("main"), None);
    assert_eq!(DesktopSurfaceKind::from_runtime_kind("iframe"), None);
}
