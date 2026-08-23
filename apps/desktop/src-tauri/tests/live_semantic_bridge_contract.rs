#![forbid(unsafe_code)]

#[test]
fn preview_bridge_normalizes_semantic_and_geometry_events() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("geometry_changed: 'layout'"));
    assert!(source.contains("semantic_snapshot: 'semantic_snapshot'"));
    assert!(source.contains("case 'inspect':"));
    assert!(source.contains("api?.inspect?.(queued.reference)"));
}
