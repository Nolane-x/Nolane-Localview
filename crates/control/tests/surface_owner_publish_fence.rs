#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "required surface publish source {} is unavailable: {error}",
            path.display()
        )
    })
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split(start)
        .nth(1)
        .unwrap_or_else(|| panic!("missing start marker {start}"))
        .split(end)
        .next()
        .unwrap_or_else(|| panic!("missing end marker {end}"))
}

#[test]
fn surface_publish_revalidates_owner_after_durable_journal_await() {
    let runtime = source("src/resource_runtime.rs");

    let activate = between(
        &runtime,
        "async fn activate_surface_resource",
        "async fn reattach_surface_resource",
    );
    let activate_publish_window = between(
        activate,
        "record_activated(recovery_key).await",
        "entry.live.insert",
    );
    assert!(
        activate_publish_window.contains("validate_surface_owner_for_sessions"),
        "activation must revalidate the exact owner after the durable journal await and before publishing a live lease"
    );

    let reattach = between(
        &runtime,
        "async fn reattach_surface_resource",
        "async fn update_surface_visibility",
    );
    let reattach_publish_window = between(
        reattach,
        "record_reattached(recovery_key).await",
        "entry.live.insert",
    );
    assert!(
        reattach_publish_window.contains("validate_surface_owner_for_sessions"),
        "reattach must revalidate the exact owner after the durable journal await and before publishing a live lease"
    );
}
