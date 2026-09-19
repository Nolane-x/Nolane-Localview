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

#[test]
fn rejected_second_xhr_send_cannot_release_the_first_request() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("const startedHere = !meta.active;"));
    assert!(script.contains("if (startedHere) {"));
    assert!(script.contains("if (startedHere && meta.active) {"));
}

#[test]
fn rejected_xhr_send_does_not_mutate_or_leak_completion_metadata() {
    let script = bootstrap_script(&InstrumentationConfig::default());
    let send = script
        .split("XMLHttpRequest.prototype.send = function(...args) {")
        .nth(1)
        .expect("XHR send wrapper must exist");

    let guard = send
        .find("if (meta.active || meta.faultPending) {")
        .expect("active/pending send guard must exist");
    let select_rule = send
        .find("const rule = selectNetworkFaultRule")
        .expect("fault selection must exist");
    let started_block = send
        .find("if (startedHere) {")
        .expect("first-send accounting block must exist");
    let started_at = send
        .find("meta.started = performance.now();")
        .expect("first send must own its start timestamp");
    let active_at = send
        .find("meta.active = true;")
        .expect("first send must become active");
    let begin = send
        .find("beginNetworkRequest();")
        .expect("first send must increment network accounting");
    let store = send
        .find("xhrMeta.set(this, meta);")
        .expect("owned metadata must be retained");

    assert!(
        guard < select_rule && select_rule < started_block,
        "a rejected second send must fail before fault selection or first-send mutation"
    );
    assert!(
        started_block < started_at && started_at < active_at && active_at < begin && begin < store,
        "first-send metadata/accounting ordering must remain explicit"
    );
    assert!(send.contains("let onLoadEnd = null;"));
    assert!(send.contains("onLoadEnd = () => {"));
    assert!(send.contains("this.addEventListener('loadend', onLoadEnd, { once: true });"));
    assert!(send.contains(
        "if (startedHere && onLoadEnd) {\n          this.removeEventListener('loadend', onLoadEnd);\n        }"
    ));
}
