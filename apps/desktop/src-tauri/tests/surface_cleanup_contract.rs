#![forbid(unsafe_code)]

use std::fs;

use localview_desktop::workspace_surface::surface_registry::{
    DesktopSurfaceKind, DesktopSurfaceRegistry, DesktopSurfaceVisibility,
};
use localview_protocol::SessionId;

fn source(path: &str) -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/").to_owned() + path)
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing start marker {start}"));
    let tail = &source[start..];
    let end = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing end marker {end}"));
    &tail[..end]
}

fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn assert_activation_close_is_fail_closed(section: &str, close_call: &str, owner_name: &str) {
    let section = compact(section);
    let activation = section
        .find("activate_surface(")
        .map(|index| &section[index..])
        .expect("surface activation call");
    let ignored_close = format!("let_={close_call};");
    assert!(
        !activation.contains(&ignored_close),
        "{owner_name} activation rollback must not ignore platform close failure"
    );
    let propagated_close = format!("{close_call}.map_err");
    let explicit_close_error = format!("ifletErr(close_error)={close_call}");
    assert!(
        activation.contains(&propagated_close) || activation.contains(&explicit_close_error),
        "{owner_name} activation rollback must fail closed when the physical surface cannot close"
    );
    let close = activation
        .find(close_call)
        .expect("platform close in activation rollback");
    let owner_cleanup = activation
        .find("record_closed(")
        .expect("owner cleanup in activation rollback");
    assert!(
        close < owner_cleanup,
        "{owner_name} owner truth may only be removed after physical close succeeds"
    );
}

fn assert_record_created_close_is_fail_closed(section: &str, close_call: &str, owner_name: &str) {
    let section = compact(section);
    let record_created = section
        .find("registry.record_created(")
        .map(|index| &section[index..])
        .expect("surface owner record_created call");
    let activation = record_created
        .find("activate_surface(")
        .expect("surface activation after owner record");
    let rollback = &record_created[..activation];

    let ignored_close = format!("let_={close_call};");
    assert!(
        !rollback.contains(&ignored_close),
        "{owner_name} record_created rollback must not ignore platform close failure"
    );
    let propagated_close = format!("{close_call}.map_err");
    let explicit_close_error = format!("ifletErr(close_error)={close_call}");
    assert!(
        rollback.contains(&propagated_close) || rollback.contains(&explicit_close_error),
        "{owner_name} record_created rollback must preserve pending authority when the physical surface cannot close"
    );
    let close = rollback
        .find(close_call)
        .expect("platform close in record_created rollback");
    let pending_cleanup = rollback
        .find("cancel_surface_reservation(")
        .expect("pending authority cleanup in record_created rollback");
    assert!(
        close < pending_cleanup,
        "{owner_name} pending central authority may only be cancelled after physical close succeeds"
    );
}

#[test]
fn preview_activation_rollback_preserves_owner_truth_when_platform_close_fails() {
    let lib = source("lib.rs");
    let open = between(
        &lib,
        "async fn open_preview(",
        "fn install_preview_surface_destroyed_reconciler(",
    );
    assert_activation_close_is_fail_closed(open, "window.close()", "preview window");
}

#[test]
fn workspace_activation_rollback_preserves_owner_truth_when_platform_close_fails() {
    let workspace = source("workspace_surface.rs");
    let open = between(&workspace, "async fn open_native(", "fn set_native_bounds(");
    assert_activation_close_is_fail_closed(open, "webview.close()", "workspace child");
}

#[test]
fn preview_record_created_rollback_preserves_pending_authority_when_platform_close_fails() {
    let lib = source("lib.rs");
    let open = between(
        &lib,
        "async fn open_preview(",
        "fn install_preview_surface_destroyed_reconciler(",
    );
    assert_record_created_close_is_fail_closed(open, "window.close()", "preview window");
}

#[test]
fn workspace_record_created_rollback_preserves_pending_authority_when_platform_close_fails() {
    let workspace = source("workspace_surface.rs");
    let open = between(&workspace, "async fn open_native(", "fn set_native_bounds(");
    assert_record_created_close_is_fail_closed(open, "webview.close()", "workspace child");
}

#[test]
fn repeated_preview_and_workspace_owner_cycles_return_to_registry_baseline() {
    let registry = DesktopSurfaceRegistry::default();
    let session_id: SessionId = "550e8400-e29b-41d4-a716-446655440000"
        .parse()
        .expect("valid session id");

    for cycle in 0..32 {
        for (kind, label) in [
            (
                DesktopSurfaceKind::PreviewWindow,
                "preview-cleanup-contract",
            ),
            (
                DesktopSurfaceKind::WorkspaceChild,
                "workspace-cleanup-contract",
            ),
        ] {
            let identity = registry.next_identity(session_id, kind, label);
            assert_eq!(identity.incarnation, cycle + 1);
            registry
                .record_created(identity.clone(), DesktopSurfaceVisibility::Hidden)
                .expect("record physical surface owner");
            registry
                .record_closed(&identity)
                .expect("record exact physical surface close");
        }
    }

    assert_eq!(
        registry.live_count(),
        0,
        "desktop surface owner ledger must converge to its zero-live baseline"
    );
}
