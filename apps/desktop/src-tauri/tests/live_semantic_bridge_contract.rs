#![forbid(unsafe_code)]

#[test]
fn preview_bridge_normalizes_semantic_and_geometry_events() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("geometry_changed: 'layout'"));
    assert!(source.contains("semantic_snapshot: 'semantic_snapshot'"));
    assert!(source.contains("case 'snapshot':"));
    assert!(source.contains("window.__LOCALVIEW__?.snapshot?.()"));
}

#[test]
fn preview_bridge_executes_internal_visual_freeze_actions_through_localview_api() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("case 'freeze_visuals':"));
    assert!(source.contains("freezeVisuals?.(queued.id)"));
    assert!(source.contains("case 'restore_visuals':"));
    assert!(source.contains("restoreVisuals?.(String(action.token"));
    assert!(!source.contains("eval(action"));
    assert!(!source.contains("evaluate_script(action"));
}

#[test]
fn preview_action_drain_prioritizes_private_capture_actions_over_public_backlog() {
    let source = include_str!("../src/lib.rs");
    let internal = source
        .find("/capture-actions")
        .expect("managed preview must drain a private capture-action channel");
    let public = source[internal..]
        .find("/actions\"")
        .map(|offset| offset + internal)
        .expect("managed preview must still drain normal public actions");

    assert!(internal < public, "capture actions must be fetched before public actions");
    assert!(source.contains("internal_actions"));
    assert!(source.contains("public_actions"));
}
