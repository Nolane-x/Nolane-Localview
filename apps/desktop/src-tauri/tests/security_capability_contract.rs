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
    let mut offset = 0;
    while let Some(relative) = source[offset..].find("invoke") {
        let invoke_start = offset + relative;
        let mut cursor = invoke_start + "invoke".len();
        while source[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }

        if source[cursor..].starts_with('<') {
            let mut depth = 0_u32;
            let mut closed = None;
            for (relative, ch) in source[cursor..].char_indices() {
                match ch {
                    '<' => depth += 1,
                    '>' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            closed = Some(cursor + relative + ch.len_utf8());
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(next) = closed else {
                offset = invoke_start + "invoke".len();
                continue;
            };
            cursor = next;
            while source[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
                cursor += source[cursor..].chars().next().unwrap().len_utf8();
            }
        }

        if !source[cursor..].starts_with('(') {
            offset = invoke_start + "invoke".len();
            continue;
        }
        cursor += 1;
        while source[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            cursor += source[cursor..].chars().next().unwrap().len_utf8();
        }

        let Some(quote) = source[cursor..]
            .chars()
            .next()
            .filter(|ch| matches!(ch, '\'' | '"'))
        else {
            offset = invoke_start + "invoke".len();
            continue;
        };
        cursor += quote.len_utf8();
        if let Some(end) = source[cursor..].find(quote) {
            out.insert(source[cursor..cursor + end].to_owned());
        }
        offset = invoke_start + "invoke".len();
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
fn invoke_parser_catches_multiline_and_generic_calls() {
    let source = r#"
        invoke<Receipt>(
            'generic_multiline',
            { value: true },
        );
        invoke (
            "spaced_multiline",
            {},
        );
    "#;
    assert_eq!(
        invoke_names(source),
        BTreeSet::from([
            "generic_multiline".to_owned(),
            "spaced_multiline".to_owned(),
        ])
    );
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
    let invokes = invoke_names(api);
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
    assert_eq!(
        csp["frame-src"],
        "'self' http://localhost:* http://127.0.0.1:* http://[::1]:* https://localhost:* https://127.0.0.1:* https://[::1]:*"
    );
    assert!(!csp.values().any(|value| value.as_str().is_some_and(|s| s.contains("'unsafe-eval'"))));
    assert!(!csp.values().any(|value| value.as_str().is_some_and(|s| s.contains(" *"))));
    let frame_src = csp["frame-src"].as_str().unwrap();
    assert!(!frame_src.contains("://*"));
    assert!(!frame_src.contains("http: *"));
    assert!(!frame_src.contains("https: *"));
    assert_eq!(security["devCsp"], serde_json::Value::Null);
    assert_eq!(
        config["app"]["withGlobalTauri"],
        true,
        "the managed initialization bootstrap captures Tauri invoke before untrusted page code"
    );
}

#[test]
fn remote_preview_bridge_requires_rust_owned_attestation() {
    let source = include_str!("../src/lib.rs");
    let bridge = source
        .split("const PREVIEW_BRIDGE_SCRIPT: &str = r#\"")
        .nth(1)
        .expect("preview bridge script must exist")
        .split("\"#;")
        .next()
        .expect("preview bridge script terminator");

    assert!(
        !bridge.contains("window.__TAURI__"),
        "managed bridge must use the captured invoke primitive rather than re-reading a page global"
    );
    assert_eq!(
        bridge.matches("window.__LOCALVIEW__").count(),
        1,
        "managed bridge may capture the trusted instrumentation object once, but must never re-read the page-replaceable global"
    );
    assert!(
        bridge.contains("const localviewApi = window.__LOCALVIEW__;"),
        "managed bridge must capture the trusted instrumentation object before application scripts run"
    );
    assert!(
        bridge.contains("const api = localviewApi;"),
        "bridge event draining must use the captured instrumentation authority"
    );
    assert!(bridge.contains("installBridge((invoke, bridgeAttestation) =>"));
    assert!(source.contains("struct PreviewBridgeAuthority"));
    assert!(source.contains("bridge_authority.verify(&current.identity, attestation)?"));

    for command in [
        "preview_ingest",
        "preview_take_actions",
        "preview_take_network_fault_controls",
        "preview_complete_network_fault_control",
        "preview_complete_action",
        "preview_complete_content_stress",
        "preview_complete_point_select",
        "preview_action_cancellation",
        "preview_ack_action_cancellation",
    ] {
        let marker = format!("invoke('{command}'");
        let offset = bridge
            .find(&marker)
            .unwrap_or_else(|| panic!("{command} invocation must remain in the managed bridge"));
        let tail = &bridge[offset..bridge.len().min(offset + 512)];
        assert!(
            tail.contains("attestation: bridgeAttestation"),
            "{command} must carry the non-page bridge attestation"
        );
    }
}

#[test]
fn managed_remote_navigation_matches_tauri_bridge_url_authority() {
    let surface = include_str!("../src/workspace_surface.rs");
    let preview: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/preview-bridge.json")).unwrap();
    let urls = preview["remote"]["urls"]
        .as_array()
        .expect("preview remote URL patterns")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();

    assert_eq!(
        urls,
        vec!["http://localhost:*/*", "http://127.0.0.1:*/*"]
    );
    assert!(surface.contains("url.scheme() != \"http\""));
    assert!(surface.contains("address == std::net::Ipv4Addr::LOCALHOST"));
    assert!(surface.contains("Some(url::Host::Ipv6(_)) | None => false"));
}


#[test]
fn bridge_completion_helpers_are_not_independent_tauri_commands() {
    let desktop = include_str!("../src/lib.rs");
    let content_stress = include_str!("../src/content_stress.rs");
    let point_select = include_str!("../src/point_select.rs");

    assert!(
        desktop.contains("#[tauri::command]\nasync fn preview_complete_content_stress("),
        "attested content-stress wrapper must remain the registered Tauri command"
    );
    assert!(
        desktop.contains("#[tauri::command]\nasync fn preview_complete_point_select("),
        "attested point-select wrapper must remain the registered Tauri command"
    );

    assert!(
        content_stress.contains("pub(super) async fn complete_content_stress_from_managed_bridge(")
            && !content_stress.contains("#[tauri::command]\npub async fn preview_complete_content_stress("),
        "content-stress helper must not expose an unattested Tauri IPC surface"
    );
    assert!(
        point_select.contains("pub(super) async fn complete_point_select_from_managed_bridge(")
            && !point_select.contains("#[tauri::command]\npub async fn preview_complete_point_select("),
        "point-select helper must not expose an unattested Tauri IPC surface"
    );
}
