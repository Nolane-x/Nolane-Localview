use std::fs;

fn desktop_source() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("desktop lib source")
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).unwrap_or_else(|| panic!("missing start marker {start}"));
    let tail = &source[start..];
    let end = tail.find(end).unwrap_or_else(|| panic!("missing end marker {end}"));
    &tail[..end]
}

fn without_layout_whitespace(value: &str) -> String {
    value.chars().filter(|character| !character.is_whitespace()).collect()
}

fn assert_in_order(haystack: &str, needles: &[&str]) {
    let haystack = without_layout_whitespace(haystack);
    let mut cursor = 0;
    for needle in needles {
        let needle = without_layout_whitespace(needle);
        let offset = haystack[cursor..]
            .find(&needle)
            .unwrap_or_else(|| panic!("missing ordered preview authority step {needle}"));
        cursor += offset + needle.len();
    }
}

#[test]
fn existing_preview_show_reconciles_exact_owner_then_central_visibility() {
    let source = desktop_source();
    let open = between(&source, "async fn open_preview(", "async fn preview_ingest(");

    assert!(
        open.contains("DesktopSurfaceRegistry"),
        "preview command must consume the one shared desktop owner registry"
    );
    assert_in_order(
        open,
        &[
            "app.get_webview_window",
            "registry.current",
            "DesktopSurfaceKind::PreviewWindow",
            "window.show()",
            "set_visibility(",
            "update_surface_visibility(",
        ],
    );
}

#[test]
fn new_preview_is_admitted_before_build_then_recorded_and_activated() {
    let source = desktop_source();
    let open = between(&source, "async fn open_preview(", "async fn preview_ingest(");

    assert_in_order(
        open,
        &[
            "next_identity(",
            "DesktopSurfaceKind::PreviewWindow",
            "reserve_surface(",
            "WebviewWindowBuilder::new",
            ".build()",
            "record_created(",
            "activate_surface(",
        ],
    );
    assert!(
        open.contains("cancel_surface_reservation("),
        "failed preview build/admission must expose exact pending-reservation cancellation"
    );
}

#[test]
fn preview_activation_failure_closes_platform_owner_before_local_cleanup() {
    let source = desktop_source();
    let open = between(&source, "async fn open_preview(", "async fn preview_ingest(");
    let activation = open
        .find("activate_surface(")
        .map(|index| &open[index..])
        .expect("preview activation call");

    assert_in_order(
        activation,
        &["if let Err", "window.close()", "record_closed("],
    );
}

#[test]
fn preview_destroy_listener_releases_only_after_platform_destroyed_event() {
    let source = desktop_source();
    assert!(
        source.contains("on_window_event"),
        "preview owner must subscribe to the concrete Tauri window lifecycle"
    );
    assert_in_order(
        &source,
        &[
            "WindowEvent::Destroyed",
            "record_closed(",
            "release_surface(",
        ],
    );
}

#[test]
fn main_close_to_tray_stays_unregistered_and_unchanged() {
    let source = desktop_source();
    let main_events = between(
        &source,
        ".on_window_event(|window, event|",
        ".invoke_handler(tauri::generate_handler!",
    );

    assert_in_order(
        main_events,
        &[
            "window.label() == \"main\"",
            "WindowEvent::CloseRequested",
            "api.prevent_close()",
            "window.hide()",
        ],
    );
    assert!(
        !main_events.contains("DesktopSurfaceKind::PreviewWindow")
            && !main_events.contains("record_created(")
            && !main_events.contains("reserve_surface("),
        "main LocalView shell must never enter target-surface resource authority"
    );
}
