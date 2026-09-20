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
        .find("const react = reactSourceHint(el, ownershipBudget)")
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
        .find("const svelte = svelteSourceHint(el, ownershipBudget)")
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

#[test]
fn vue_ownership_is_exact_element_bounded_and_does_not_fabricate_source_coordinates() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "MAX_FRAMEWORK_OWNERSHIP_PROBES = 256",
        "MAX_VUE_COMPONENT_BYTES = 96",
        "MAX_VUE_SOURCE_FILE_BYTES = 260",
        "MAX_VUE_ABSOLUTE_SOURCE_FILE_BYTES = 1024",
        "boundedVueSourceFile(fileDescriptor.value)",
        "ownDataDescriptor(el, '__vueParentComponent')",
        "ownDataDescriptor(instance, 'type')",
        "ownDataDescriptor(componentType, '__file')",
        "!file.endsWith('.vue')",
        "origin: 'vue-dev-instance'",
        "signal: 'element_parent_component'",
        "vueSourceHint(el, ownershipBudget)",
    ] {
        assert!(
            script.contains(required),
            "missing bounded Vue ownership contract: {required}"
        );
    }

    let explicit_index = script
        .find("for (const attribute of ['data-component-source', 'data-source'])")
        .expect("explicit source precedence");
    let react_index = script
        .find("const react = reactSourceHint(el, ownershipBudget)")
        .expect("React precedence");
    let svelte_index = script
        .find("const svelte = svelteSourceHint(el, ownershipBudget)")
        .expect("Svelte precedence");
    let vue_index = script
        .find("return vueSourceHint(el, ownershipBudget)")
        .expect("Vue fallback");
    assert!(
        explicit_index < react_index && react_index < svelte_index && svelte_index < vue_index,
        "explicit source must outrank React, Svelte and Vue framework fallback"
    );

    for forbidden in [
        "instance.props",
        "instance.attrs",
        "instance.slots",
        "instance.setupState",
        "instance.ctx",
        "instance.proxy",
        "instance.exposed",
        "instance.parent",
        "instance.subTree",
        "__VUE_DEVTOOLS_GLOBAL_HOOK__",
    ] {
        assert!(
            !script.contains(forbidden),
            "Vue ownership must not read component runtime state: {forbidden}"
        );
    }
}


#[test]
fn css_declaration_trace_is_bounded_privacy_safe_and_separate_from_component_ownership() {
    let script = bootstrap_script(&InstrumentationConfig::default());

    for required in [
        "CSS_TRACE_PROPERTIES",
        "MAX_CSS_TRACE_STYLESHEETS = 96",
        "MAX_CSS_TRACE_RULES = 512",
        "MAX_CSS_TRACE_DECLARATIONS = 12",
        "MAX_CSS_SELECTOR_BYTES = 256",
        "MAX_CSS_VALUE_BYTES = 256",
        "MAX_CSS_SOURCE_FILE_BYTES = 260",
        "value.replace(/url\\([^)]*\\)/gi, 'url(<redacted>)')",
        "Array.from(document.styleSheets || []).slice(0, MAX_CSS_TRACE_STYLESHEETS)",
        "rules = Array.from(sheet.cssRules || [])",
        "matches = el.matches(selector)",
        "styleTrace: includeStyle ? cssDeclarationTrace(el) : null",
    ] {
        assert!(
            script.contains(required),
            "missing bounded CSS trace contract: {required}"
        );
    }

    assert!(
        script.find("sourceHint: sourceHint(el, ownershipBudget)").unwrap()
            < script.find("styleTrace: includeStyle ? cssDeclarationTrace(el) : null").unwrap(),
        "CSS declaration evidence must remain separate from component/source ownership"
    );
    assert!(
        script.contains("url.origin !== location.origin"),
        "stylesheet file identity must stay same-origin"
    );
    assert!(
        script.contains("url.pathname.startsWith('/@fs/')"),
        "filesystem-backed browser paths must not become retained CSS file identity"
    );
}
