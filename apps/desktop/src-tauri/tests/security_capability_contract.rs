#![forbid(unsafe_code)]

use std::collections::BTreeSet;

fn permission_commands(permission: &str) -> BTreeSet<String> {
    let source = include_str!("../permissions/localview.toml");
    let block = source
        .split(&format!("identifier = \"{permission}\""))
        .nth(1)
        .unwrap_or_else(|| panic!("permission {permission} must exist"))
        .split("[[permission]]")
        .next()
        .expect("permission block");
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with('"') {
                return None;
            }
            Some(
                line.trim_end_matches(',')
                    .trim_matches('"')
                    .to_owned(),
            )
        })
        .collect()
}

fn invoke_names(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for quote in ['\'', '"'] {
        let marker = format!("invoke({quote}");
        let mut tail = source;
        while let Some(index) = tail.find(&marker) {
            let rest = &tail[index + marker.len()..];
            if let Some(end) = rest.find(quote) {
                out.insert(rest[..end].to_owned());
                tail = &rest[end + quote.len_utf8()..];
            } else {
                break;
            }
        }
    }
    out
}

fn api_invoke_names(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in source.lines() {
        let Some(invoke) = line.find("invoke") else { continue };
        let tail = &line[invoke + "invoke".len()..];
        let Some(open) = tail.find('(') else { continue };
        let rest = tail[open + 1..].trim_start();
        let Some(quote) = rest.chars().next().filter(|ch| matches!(ch, '\'' | '"')) else {
            continue;
        };
        let quoted = &rest[quote.len_utf8()..];
        if let Some(end) = quoted.find(quote) {
            out.insert(quoted[..end].to_owned());
        }
    }
    out
}

fn registered_handlers() -> BTreeSet<String> {
    let source = include_str!("../src/lib.rs");
    let block = source
        .split(".invoke_handler(tauri::generate_handler![")
        .nth(1)
        .expect("Tauri generate_handler block must exist")
        .split("])")
        .next()
        .expect("Tauri generate_handler block terminator");
    block
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.rsplit("::").next().unwrap_or(entry).trim().to_owned())
        .collect()
}

#[test]
fn preview_bridge_invokes_are_registered_and_least_privilege_allowed() {
    let source = include_str!("../src/lib.rs");
    let bridge = source
        .split("const PREVIEW_BRIDGE_SCRIPT: &str = r#\"")
        .nth(1)
        .expect("preview bridge script must exist")
        .split("\"#;")
        .next()
        .expect("preview bridge script terminator");
    let invokes = invoke_names(bridge);
    let handlers = registered_handlers();
    let preview = permission_commands("previewbridge");
    let main = permission_commands("maincommands");

    let expected = BTreeSet::from([
        "preview_ingest".to_owned(),
        "preview_take_actions".to_owned(),
        "preview_take_network_fault_controls".to_owned(),
        "preview_complete_network_fault_control".to_owned(),
        "preview_complete_action".to_owned(),
        "preview_complete_content_stress".to_owned(),
        "preview_complete_point_select".to_owned(),
        "preview_action_cancellation".to_owned(),
        "preview_ack_action_cancellation".to_owned(),
    ]);
    assert_eq!(invokes, expected, "bridge command set changed; review capability authority");
    assert!(invokes.iter().all(|command| handlers.contains(command)));
    assert_eq!(preview, invokes, "previewbridge must grant exactly bridge invocations");
    assert!(preview.is_disjoint(&main), "preview commands must not leak into maincommands");
}

#[test]
fn bundled_frontend_invokes_are_registered_and_main_capability_allowed() {
    let api = include_str!("../../src/api.ts");
    let invokes = api_invoke_names(api);
    let handlers = registered_handlers();
    let main = permission_commands("maincommands");

    assert!(invokes.contains("capture_responsive_sweep"));
    assert!(invokes.iter().all(|command| handlers.contains(command)));
    assert!(
        invokes.iter().all(|command| main.contains(command)),
        "every production frontend invoke must be explicitly allowed by maincommands"
    );

    for hidden_capture in [
        "capture_full_page",
        "capture_region",
        "capture_changed_regions",
        "capture_progressive_target",
        "capture_visual_packet",
    ] {
        assert!(
            !main.contains(hidden_capture),
            "dashboard must not gain unused capture authority: {hidden_capture}"
        );
    }
}

#[test]
fn capability_files_keep_preview_and_dashboard_authority_disjoint() {
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
    let preview: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/preview-bridge.json")).unwrap();

    let main_permissions = main["permissions"].as_array().unwrap();
    let preview_permissions = preview["permissions"].as_array().unwrap();
    assert!(main_permissions.iter().any(|value| value == "core:default"));
    assert!(main_permissions.iter().any(|value| value == "maincommands"));
    assert!(!main_permissions.iter().any(|value| value == "previewbridge"));
    assert_eq!(preview_permissions.len(), 1);
    assert_eq!(preview_permissions[0], "previewbridge");
    assert!(!preview_permissions.iter().any(|value| value == "core:default"));
    assert!(!preview_permissions.iter().any(|value| value == "maincommands"));
}

#[test]
fn production_csp_is_non_null_and_script_policy_stays_strict() {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let security = &config["app"]["security"];
    let csp = security["csp"].as_object().expect("production CSP must be configured");

    assert_eq!(csp["script-src"], "'self'");
    assert!(!csp.values().any(|value| value.as_str().is_some_and(|s| s.contains("'unsafe-eval'"))));
    assert!(!csp.values().any(|value| value.as_str().is_some_and(|s| s.contains(" *"))));
    assert_eq!(security["devCsp"], serde_json::Value::Null);
    assert_eq!(config["app"]["withGlobalTauri"], true, "preview bridge currently requires window.__TAURI__");
}
