#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("required source {} is unavailable: {error}", path.display())
    })
}

fn function_body<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source
        .find(start)
        .unwrap_or_else(|| panic!("missing function marker {start}"));
    let tail = &source[start_index..];
    let end_index = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing function end marker {end}"));
    &tail[..end_index]
}

fn assert_ordered(haystack: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let relative = haystack[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing ordered transaction step {needle}"));
        cursor += relative + needle.len();
    }
}

#[test]
fn desktop_manages_one_shared_surface_owner_registry() {
    let desktop = source("src/lib.rs");
    assert!(
        desktop.contains(
            ".manage(workspace_surface::surface_registry::DesktopSurfaceRegistry::default())"
        ),
        "desktop must manage exactly one shared owner registry for preview/workspace surfaces"
    );
    assert_eq!(
        desktop.matches("DesktopSurfaceRegistry::default()").count(),
        1,
        "desktop must not create duplicate owner registries"
    );
}

#[test]
fn new_native_workspace_surface_is_admitted_before_platform_create_then_activated() {
    let workspace = source("src/workspace_surface.rs");
    let open = function_body(&workspace, "fn open_native(", "fn set_native_bounds(");

    assert_ordered(
        open,
        &[
            "registry.next_identity",
            "surface_resource::reserve_surface",
            ".add_child(",
            "registry.record_created",
            "surface_resource::activate_surface",
        ],
    );
    assert!(
        open.contains("surface_resource::cancel_surface_reservation(&reservation)"),
        "failed platform creation must cancel its exact pending resource reservation"
    );
    assert!(
        open.contains("webview.close()") && open.contains("registry.record_closed"),
        "activation failure must close the just-created child and remove local owner truth"
    );
}

#[test]
fn existing_native_workspace_show_updates_exact_owner_then_central_visibility() {
    let workspace = source("src/workspace_surface.rs");
    let open = function_body(&workspace, "fn open_native(", "fn set_native_bounds(");

    assert_ordered(
        open,
        &[
            "webview.show()",
            "registry.set_visibility",
            "surface_resource::update_surface_visibility",
        ],
    );
    assert!(
        open.contains("surface_registry::DesktopSurfaceKind::WorkspaceChild"),
        "existing platform child must resolve the current exact workspace owner identity"
    );
}

#[test]
fn native_workspace_close_releases_only_after_platform_and_owner_close() {
    let workspace = source("src/workspace_surface.rs");
    let close = function_body(&workspace, "fn close_native(", "#[cfg(test)]");

    assert_ordered(
        close,
        &[
            "webview.close()",
            "registry.record_closed",
            "surface_resource::release_surface",
        ],
    );
    assert!(
        close.contains("surface_registry::DesktopSurfaceKind::WorkspaceChild"),
        "close must resolve and release the exact current workspace incarnation"
    );
}
