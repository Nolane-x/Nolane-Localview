use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

fn script() -> String {
    bootstrap_script(&InstrumentationConfig::default())
}

#[test]
fn network_fault_lease_is_private_bounded_and_expiring() {
    let script = script();

    assert!(script.contains("let networkFaultLease = null;"));
    assert!(script.contains("const installNetworkFaultPlan ="));
    assert!(script.contains("const clearNetworkFaultPlan ="));
    assert!(script.contains("const networkFaultState ="));
    assert!(script.contains("expiresAt: performance.now() + plan.lease_ms"));
    assert!(script.contains("if (performance.now() >= networkFaultLease.expiresAt)"));
    assert!(script.contains("hits: 0"));
    assert!(script.contains("rule.hits >= rule.max_hits"));
    assert!(script.contains("networkFaultLease = null;"));

    assert!(script.contains("installNetworkFaultPlan,"));
    assert!(script.contains("clearNetworkFaultPlan,"));
    assert!(script.contains("networkFaultState,"));
}

#[test]
fn runtime_not_caller_owns_loopback_target_normalization() {
    let script = script();

    assert!(script.contains("const normalizeNetworkFaultTarget ="));
    assert!(script.contains("new URL(String(rawUrl || ''), location.href)"));
    assert!(script.contains("faultUrl.protocol !== 'http:'"));
    assert!(script.contains("faultUrl.protocol !== 'https:'"));
    assert!(script.contains("hostname === 'localhost'"));
    assert!(script.contains("hostname.endsWith('.localhost')"));
    assert!(script.contains("hostname.startsWith('127.')"));
    assert!(script.contains("hostname === '::1'"));
    assert!(script.contains("hostname === '[::1]'"));
    assert!(script.contains("path: faultUrl.pathname"));

    assert!(!script.contains("faultUrl.search"));
    assert!(!script.contains("faultUrl.hash"));
}

#[test]
fn fetch_faults_use_one_accounting_lifecycle_and_bounded_effects() {
    let script = script();

    assert!(script.contains("const selectNetworkFaultRule ="));
    assert!(script.contains("transport !== 'both' && transport !== requestedTransport"));
    assert!(script.contains("rule.hits += 1;"));
    assert!(script.contains("rule.effect.kind === 'fail'"));
    assert!(script.contains("rule.effect.kind === 'delay'"));
    assert!(script.contains("rule.effect.kind === 'mock_status'"));
    assert!(script.contains("LocalView injected network failure"));
    assert!(script.contains(
        "await new Promise(resolve => setTimeout(resolve, rule.effect.milliseconds));"
    ));
    assert!(script.contains("new Response(null, { status: rule.effect.status })"));
    assert!(script.contains("faultInjected: Boolean(rule)"));
    assert!(script.contains("faultRuleId: rule?.id || null"));
    assert!(script.contains("faultEffect: rule?.effect?.kind || null"));
    assert!(script.contains(
        "faultDelayMs: rule?.effect?.kind === 'delay' ? rule.effect.milliseconds : null"
    ));
    assert!(script.contains(
        "faultStatus: rule?.effect?.kind === 'mock_status' ? rule.effect.status : null"
    ));

    let fetch_block = script
        .split("window.fetch = async (...args) => {")
        .nth(1)
        .expect("fetch wrapper")
        .split("const xhrMeta = new WeakMap();")
        .next()
        .expect("fetch block");
    assert_eq!(fetch_block.matches("beginNetworkRequest();").count(), 1);
    assert_eq!(fetch_block.matches("finishNetworkRequest();").count(), 1);
    assert!(fetch_block.contains("finally"));
}

#[test]
fn xhr_faults_complete_exactly_once_without_native_double_completion() {
    let script = script();

    assert!(script.contains("const completeSyntheticXhrFault ="));
    assert!(script.contains("meta.faultPending"));
    assert!(script.contains("if (meta.active || meta.faultPending)"));
    assert!(script.contains("Object.defineProperty(xhr, 'status'"));
    assert!(script.contains("Object.defineProperty(xhr, 'readyState'"));
    assert!(script.contains("xhr.dispatchEvent(new Event('readystatechange'))"));
    assert!(script.contains("xhr.dispatchEvent(new Event(success ? 'load' : 'error'))"));
    assert!(script.contains("xhr.dispatchEvent(new Event('loadend'))"));
    assert!(script.contains("faultCompleted"));
    assert!(script.contains("if (faultCompleted) return;"));

    assert!(script.contains("transport: 'xhr'"));
    assert!(script.contains("faultInjected: Boolean(rule)"));
    assert!(script.contains("faultDelayMs:"));
    assert!(script.contains("faultStatus:"));
}

#[test]
fn fault_matching_never_reads_private_request_or_response_content() {
    let script = script();

    for forbidden in [
        "request.headers",
        "response.headers",
        "request.body",
        "response.body",
        "response.text()",
        "response.json()",
        "document.cookie",
        "localStorage",
        "sessionStorage",
    ] {
        assert!(
            !script.contains(forbidden),
            "network fault matching must not inspect private content: {forbidden}"
        );
    }
}
