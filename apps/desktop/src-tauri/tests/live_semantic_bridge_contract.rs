#![forbid(unsafe_code)]

#[test]
fn preview_bridge_normalizes_semantic_and_geometry_events() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("geometry_changed: 'layout'"));
    assert!(source.contains("semantic_snapshot: 'semantic_snapshot'"));
    assert!(source.contains("case 'snapshot':"));
    assert!(source.contains("window.__LOCALVIEW__?.snapshot?.()"));
}
