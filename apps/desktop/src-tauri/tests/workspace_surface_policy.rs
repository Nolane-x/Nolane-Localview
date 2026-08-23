#![forbid(unsafe_code)]

use localview_desktop::workspace_surface::{
    bridge_surface_label_allowed, validate_workspace_bounds, workspace_label, WorkspaceBounds,
};
use localview_protocol::SessionId;

fn session(value: &str) -> SessionId {
    value.parse().expect("valid UUID")
}

#[test]
fn workspace_labels_are_deterministic_and_separate_from_preview_labels() {
    let id = session("550e8400-e29b-41d4-a716-446655440000");
    let label = workspace_label(id);

    assert!(label.starts_with("workspace-"));
    assert!(label.chars().all(|character| character.is_ascii_alphanumeric() || character == '-'));
    assert_eq!(label, workspace_label(id));
    assert_ne!(label, "preview-550e8400e29b41d4a7");
}

#[test]
fn bridge_surface_label_requires_exact_session_ownership() {
    let first = session("550e8400-e29b-41d4-a716-446655440000");
    let second = session("8f14e45f-ea5e-4f35-a9b0-674a52bc43ab");

    assert!(bridge_surface_label_allowed(
        "preview-550e8400e29b41d4a7",
        first
    ));
    assert!(bridge_surface_label_allowed(&workspace_label(first), first));
    assert!(!bridge_surface_label_allowed(&workspace_label(first), second));
    assert!(!bridge_surface_label_allowed("main", first));
    assert!(!bridge_surface_label_allowed("workspace-*", first));
}

#[test]
fn workspace_bounds_reject_invalid_or_unbounded_geometry() {
    let valid = WorkspaceBounds {
        x: 0.0,
        y: 0.0,
        width: 1180.0,
        height: 760.0,
    };
    assert_eq!(validate_workspace_bounds(valid), Ok(valid));

    for invalid in [
        WorkspaceBounds { width: 0.0, ..valid },
        WorkspaceBounds { height: -1.0, ..valid },
        WorkspaceBounds { x: f64::NAN, ..valid },
        WorkspaceBounds { y: f64::INFINITY, ..valid },
        WorkspaceBounds { width: 100_001.0, ..valid },
        WorkspaceBounds { height: 100_001.0, ..valid },
    ] {
        assert!(validate_workspace_bounds(invalid).is_err(), "invalid bounds were accepted: {invalid:?}");
    }
}
