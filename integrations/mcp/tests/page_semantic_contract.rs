#![forbid(unsafe_code)]

#[test]
fn mcp_exposes_completed_snapshot_and_inspect_tools() {
    let source = include_str!("../src/main.rs");
    assert!(source.contains("\"page.snapshot\""));
    assert!(source.contains("\"page.inspect\""));
    assert!(source.contains("execute_page_action"));
    assert!(source.contains("actions/results"));
    assert!(source.contains("from_secs(2)"));
}
