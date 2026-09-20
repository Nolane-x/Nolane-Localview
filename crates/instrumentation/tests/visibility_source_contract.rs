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


#[test]
fn react_ownership_is_bounded_read_only_and_lower_priority_than_explicit_sources() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "MAX_REACT_OWNERSHIP_PROBES = 256",
        "MAX_REACT_HOST_KEYS = 64",
        "MAX_REACT_FIBER_DEPTH = 32",
        "MAX_REACT_DEBUG_STACK_BYTES = 16384",
        "MAX_REACT_DEBUG_STACK_LINES = 24",
        "__reactFiber$",
        "__reactInternalInstance$",
        "fiber.stateNode !== el",
        "fiber?._debugSource",
        "fiber?._debugStack",
        "url.origin !== location.origin",
        "react-dev-fiber",
        "reactSourceHint(el, ownershipBudget)",
    ] {
        assert!(
            script.contains(required),
            "missing bounded React ownership contract: {required}"
        );
    }

    let explicit_index = script
        .find("for (const attribute of ['data-component-source', 'data-source'])")
        .expect("explicit source precedence");
    let react_fallback_index = script
        .find("return reactSourceHint(el, ownershipBudget)")
        .expect("React fallback");
    assert!(
        explicit_index < react_fallback_index,
        "explicit source hints must remain higher authority than React fallback"
    );

    for forbidden in [
        "__REACT_DEVTOOLS_GLOBAL_HOOK__ =",
        "memoizedProps",
        "pendingProps",
        "memoizedState",
        ".state =",
        ".props =",
    ] {
        assert!(
            !script.contains(forbidden),
            "React ownership must not retain or mutate private component data: {forbidden}"
        );
    }
}
