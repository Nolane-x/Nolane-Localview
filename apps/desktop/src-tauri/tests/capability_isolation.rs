#![forbid(unsafe_code)]

use serde_json::Value;

fn capability(path: &str) -> Value {
    let raw = match path {
        "main" => include_str!("../capabilities/default.json"),
        "preview" => include_str!("../capabilities/preview-bridge.json"),
        _ => panic!("unknown capability fixture: {path}"),
    };
    serde_json::from_str(raw).expect("capability JSON must parse")
}

fn strings<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn multi_webview_capabilities_target_webview_labels_not_parent_windows() {
    let main = capability("main");
    let preview = capability("preview");

    assert!(
        main.get("windows").is_none(),
        "dashboard capability must not grant every child webview of the main window"
    );
    assert!(
        preview.get("windows").is_none(),
        "remote bridge capability must be scoped by webview label"
    );

    assert_eq!(strings(&main, "webviews"), vec!["main"]);
    assert_eq!(
        strings(&preview, "webviews"),
        vec!["preview-*", "workspace-*"]
    );
}

#[test]
fn dashboard_and_remote_surface_permissions_remain_disjoint() {
    let main = capability("main");
    let preview = capability("preview");

    let main_permissions = strings(&main, "permissions");
    let preview_permissions = strings(&preview, "permissions");

    assert!(main_permissions.contains(&"core:default"));
    assert!(main_permissions.contains(&"maincommands"));
    assert!(!main_permissions.contains(&"previewbridge"));

    assert_eq!(preview_permissions, vec!["previewbridge"]);
    assert!(!preview_permissions.contains(&"core:default"));
    assert!(!preview_permissions.contains(&"maincommands"));
}

#[test]
fn remote_surface_capability_is_loopback_only() {
    let preview = capability("preview");

    assert_eq!(preview.get("local").and_then(Value::as_bool), Some(false));
    let urls = preview
        .pointer("/remote/urls")
        .and_then(Value::as_array)
        .expect("preview remote.urls must exist")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        urls,
        vec!["http://localhost:*/*", "http://127.0.0.1:*/*"]
    );
    assert!(urls.iter().all(|url| {
        url.starts_with("http://localhost:") || url.starts_with("http://127.0.0.1:")
    }));
}
