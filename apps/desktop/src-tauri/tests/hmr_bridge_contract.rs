#![forbid(unsafe_code)]

#[test]
fn preview_bridge_preserves_hmr_observer_kind() {
    let lib = include_str!("../src/lib.rs");
    let bridge = lib
        .split("const PREVIEW_BRIDGE_SCRIPT: &str = r#\"")
        .nth(1)
        .expect("preview bridge script must exist")
        .split("\"#;")
        .next()
        .expect("preview bridge script must be bounded");

    assert!(
        bridge.contains("hmr: 'hmr'"),
        "raw instrumentation HMR events must survive desktop normalization"
    );
    assert!(
        bridge.contains("const kind = eventKind(raw.type);"),
        "normalization must continue through the bounded event-kind map"
    );
    assert!(
        bridge.contains("if (!kind) return [];"),
        "unknown raw instrumentation events must remain fail-closed"
    );
}

#[test]
fn hmr_mapping_does_not_create_a_second_settle_authority() {
    let lib = include_str!("../src/lib.rs");
    let bridge = lib
        .split("const PREVIEW_BRIDGE_SCRIPT: &str = r#\"")
        .nth(1)
        .expect("preview bridge script must exist")
        .split("\"#;")
        .next()
        .expect("preview bridge script must be bounded");

    assert!(!bridge.contains("HMR_QUIET_MS"));
    assert!(!bridge.contains("hmr_settled"));
    assert!(!bridge.contains("setTimeout(() => push('hmr'"));
}
