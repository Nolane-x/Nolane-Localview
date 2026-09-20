use localview_instrumentation::{InstrumentationConfig, bootstrap_script};

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
        "MAX_FRAMEWORK_OWNERSHIP_PROBES = 256",
        "MAX_REACT_HOST_KEYS = 64",
        "MAX_REACT_FIBER_DEPTH = 32",
        "MAX_REACT_DEBUG_STACK_BYTES = 16384",
        "MAX_REACT_DEBUG_STACK_LINES = 24",
        "__reactFiber$",
        "__reactInternalInstance$",
        "__reactProps$",
        "Object.getOwnPropertyDescriptor(el, key)",
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

#[test]
fn svelte_ownership_is_exact_element_bounded_and_never_reads_parent_runtime_state() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "MAX_FRAMEWORK_OWNERSHIP_PROBES = 256",
        "MAX_SVELTE_COMPONENT_BYTES = 96",
        "MAX_SVELTE_SOURCE_FILE_BYTES = 260",
        "MAX_SVELTE_SOURCE_LINE = 1000000",
        "MAX_SVELTE_SOURCE_COLUMN = 10000000",
        "Object.getOwnPropertyDescriptor(object, key)",
        "Object.prototype.hasOwnProperty.call(descriptor, 'value')",
        "ownDataDescriptor(el, '__svelte_meta')",
        "ownDataDescriptor(meta, 'loc')",
        "ownDataDescriptor(loc, 'file')",
        "ownDataDescriptor(loc, 'line')",
        "ownDataDescriptor(loc, 'column')",
        "boundedRelativeSourceFile(fileDescriptor.value, MAX_SVELTE_SOURCE_FILE_BYTES)",
        "!file.endsWith('.svelte')",
        "origin: 'svelte-dev-meta'",
        "signal: 'element_meta'",
        "svelteSourceHint(el, ownershipBudget)",
    ] {
        assert!(
            script.contains(required),
            "missing bounded Svelte ownership contract: {required}"
        );
    }

    let explicit_index = script
        .find("for (const attribute of ['data-component-source', 'data-source'])")
        .expect("explicit source precedence");
    let react_index = script
        .find("const react = reactSourceHint(el, ownershipBudget)")
        .expect("React precedence");
    let svelte_index = script
        .find("return svelteSourceHint(el, ownershipBudget)")
        .expect("Svelte fallback");
    assert!(
        explicit_index < react_index && react_index < svelte_index,
        "explicit source must outrank React, and React must outrank Svelte fallback"
    );

    for forbidden in [
        "meta.parent",
        "__SVELTE_DEVTOOLS_GLOBAL_HOOK__",
        "svelteContext",
        "component_context",
    ] {
        assert!(
            !script.contains(forbidden),
            "Svelte ownership must not retain or traverse runtime state: {forbidden}"
        );
    }
}
