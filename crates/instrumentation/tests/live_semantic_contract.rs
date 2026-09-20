use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

#[test]
fn default_runtime_exposes_bounded_semantic_and_geometry_contract() {
    let config = InstrumentationConfig::default();
    let encoded = serde_json::to_value(&config).expect("config serializes");

    assert_eq!(encoded["max_semantic_nodes"], 600);
    assert_eq!(encoded["max_tree_depth"], 12);
    assert_eq!(encoded["max_style_nodes"], 192);
    assert_eq!(encoded["max_geometry_nodes"], 384);

    let script = bootstrap_script(&config);
    assert!(script.contains("semantic_snapshot"));
    assert!(script.contains("geometry_changed"));
    assert!(script.contains("computedStylePacket"));
    assert!(script.contains("semanticTree"));
    assert!(script.contains("inspect(reference)"));
    assert!(script.contains("documentRect"));
    assert!(script.contains("layout_changes"));
}

#[test]
fn semantic_packets_remain_privacy_bounded() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(!script.contains("response.text()"));
    assert!(!script.contains("response.json()"));
    assert!(!script.contains("backgroundImage"));
    assert!(!script.contains("cssText"));
    assert!(!script.contains("localStorage"));
    assert!(!script.contains("sessionStorage"));
    assert!(!script.contains("document.cookie"));
}

#[test]
fn semantic_snapshot_exposes_privacy_safe_capture_readiness() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("readinessPacket"));
    assert!(script.contains("document.fonts"));
    assert!(script.contains("pendingImages"));
    assert!(script.contains("document.images"));
    assert!(!script.contains("image.src"));
    assert!(!script.contains("image.currentSrc"));
}

#[test]
fn deep_semantic_names_do_not_force_inner_text_layout_scans() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(!script.contains("el.innerText"));
    assert!(script.contains("textNameAllowed"));
    assert!(script.contains("boundedText"));
    assert!(script.contains("el.textContent"));
}


#[test]
fn css_declaration_evidence_is_on_demand_and_hard_bounded() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for marker in [
        "inspectCss(reference)",
        "cssDeclarationEvidence",
        "MAX_CSS_TRACE_STYLESHEETS = 64",
        "MAX_CSS_TRACE_RULES = 512",
        "MAX_CSS_TRACE_MATCHED_RULES = 64",
        "MAX_CSS_TRACE_DECLARATIONS = 128",
        "CSS_TRACE_PROPERTIES",
        "opaque_stylesheets",
        "conditional_rules_omitted",
    ] {
        assert!(script.contains(marker), "missing CSS evidence marker: {marker}");
    }

    assert!(script.contains("sheet.cssRules"));
    assert!(script.contains("el.matches(rule.selectorText)"));
    assert!(script.contains("url.origin !== location.origin"));
    assert!(script.contains("url.pathname"));
    assert!(!script.contains("getPropertyValue('--"));
    assert!(!script.contains("rule.cssText"));
}
