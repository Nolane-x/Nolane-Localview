#[test]
fn wave6_is_wired_only_into_managed_preview_and_workspace_initialization() {
    let desktop = include_str!("../src/lib.rs");
    let workspace = include_str!("../src/workspace_surface.rs");
    let adapter = include_str!("../src/wave6_accessibility_interaction.rs");
    let tauri_config = include_str!("../tauri.conf.json");
    let package = include_str!("../../package.json");

    assert!(desktop.contains("mod wave6_accessibility_interaction;"));
    assert!(desktop.contains(
        "wave6_accessibility_interaction::managed_initialization_script(&app, session)?"
    ));
    assert!(workspace.contains(
        "super::wave6_accessibility_interaction::managed_initialization_script(app, session_id)?"
    ));

    assert!(adapter.contains("BaseDirectory::Resource"));
    assert!(adapter.contains("wave6/axe.min.js"));
    assert!(adapter.contains("resources/wave6/axe.min.js"));
    assert!(adapter.contains("wave6_bootstrap_script()"));
    assert!(adapter.contains("installAxe(window.axe)"));
    assert!(adapter.contains("MAX_AXE_SOURCE_BYTES"));

    assert!(tauri_config.contains("resources/wave6/axe.min.js"));
    assert!(tauri_config.contains("wave6/axe-core-LICENSE"));
    assert!(package.contains("\"axe-core\": \"4.13.0\""));
    let vendored_axe = include_str!("../resources/wave6/axe.min.js");
    let vendored_license = include_str!("../resources/wave6/axe-core-LICENSE");
    assert!(vendored_axe.starts_with("/*! axe v4.13.0"));
    assert!(vendored_license.starts_with("Mozilla Public License, version 2.0"));
}

#[test]
fn wave6_adapter_has_no_runtime_cdn_fallback() {
    let adapter = include_str!("../src/wave6_accessibility_interaction.rs");
    let production = adapter
        .split("#[cfg(test)]")
        .next()
        .expect("production adapter");
    assert!(!production.contains("cdn.jsdelivr"));
    assert!(!production.contains("unpkg.com"));
    assert!(!production.contains("cdnjs"));
    assert!(!production.contains("reqwest"));
}
