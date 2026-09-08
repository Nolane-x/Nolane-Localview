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

fn async_handler_body<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("async fn {name}");
    source
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("missing async surface handler {name}"))
        .split("\nasync fn ")
        .next()
        .expect("surface handler body")
}

#[test]
fn surface_lifecycle_holds_exact_owner_pin_until_transaction_finishes() {
    let runtime = source("src/resource_runtime.rs");

    for handler in [
        "reserve_surface_resource",
        "cancel_surface_reservation",
        "activate_surface_resource",
        "reattach_surface_resource",
        "update_surface_visibility",
        "release_surface_resource",
    ] {
        let body = async_handler_body(&runtime, handler);
        assert!(
            body.contains("pin_surface_owner_for_sessions"),
            "{handler} must pin the exact current owner for the full mutation transaction"
        );
    }

    assert!(
        !runtime.contains("record_reattached(recovery_key).await.is_ok()"),
        "reattach must not depend on best-effort recovery-journal compensation after debt discharge"
    );
}
