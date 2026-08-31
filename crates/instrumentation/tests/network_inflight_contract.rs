use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

#[test]
fn readiness_exposes_only_bounded_network_inflight_count() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("inflightNetworkRequests"));
    assert!(script.contains("inflightRequests:"));
    assert!(script.contains("config.include_network ? inflightNetworkRequests : null"));
    assert!(!script.contains("inflightUrls"));
    assert!(!script.contains("inflightBodies"));
    assert!(!script.contains("inflightHeaders"));
}

#[test]
fn fetch_and_xhr_account_for_start_and_completion() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("beginNetworkRequest"));
    assert!(script.contains("finishNetworkRequest"));
    assert!(script.contains("const originalFetch"));
    assert!(script.contains("XMLHttpRequest.prototype.send"));
}
