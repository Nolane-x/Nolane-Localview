use localview_instrumentation::{bootstrap_script, InstrumentationConfig};

#[test]
fn semantic_nodes_report_bounded_visibility_and_occlusion() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("elementsFromPoint"));
    assert!(script.contains("occluded"));
    assert!(script.contains("occludedBy"));
    assert!(script.contains("inViewport"));
    assert!(script.contains("clipped"));
    assert!(script.contains("max_occlusion_samples"));
}

#[test]
fn semantic_nodes_preserve_explicit_dev_source_hints_without_scanning_source_files() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    assert!(script.contains("data-source"));
    assert!(script.contains("data-component-source"));
    assert!(script.contains("sourceHint"));
    assert!(!script.contains("sourceMappingURL"));
}
