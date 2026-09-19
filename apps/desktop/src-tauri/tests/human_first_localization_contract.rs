#[test]
fn canonical_human_first_spec_requires_runtime_localization_evidence() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-18-human-first-localview-ui-ux-v2.md");

    for required in [
        "String-presence tests can overclaim behavior",
        "Partial localization",
        "Audit human-facing strings in touched V2 surfaces",
    ] {
        assert!(
            spec.contains(required),
            "Human-First V2 localization contract is missing: {required}"
        );
    }
}

#[test]
fn diagnostics_surface_routes_human_facing_copy_through_i18n() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    for required in [
        "translate(locale, 'empty.noTarget')",
        "translate(locale, 'empty.runDevServer')",
        "translate(locale, 'advanced.status')",
        "translate(locale, 'advanced.observer')",
        "translate(locale, 'advanced.notAttached')",
        "translate(locale, 'advanced.latest')",
        "translate(locale, 'advanced.focusedRef')",
        "translate(locale, 'advanced.projectIdentity')",
        "translate(locale, 'advanced.diagnostic')",
        "translate(locale, 'advanced.processDerivedIdentity')",
        "translate(locale, 'advanced.runtimePipeline')",
        "translate(locale, 'advanced.semanticRefs')",
        "translate(locale, 'advanced.geometryEvidence')",
        "translate(locale, 'advanced.sourceHints')",
        "translate(locale, 'advanced.secureObserverDrain')",
        "translate(locale, 'status.ready')",
        "translate(locale, 'status.idle')",
        "translate(locale, 'status.active')",
        "translate(locale, 'status.disconnected')",
        "translate(locale, 'status.hidden')",
        "translate(locale, 'status.closed')",
    ] {
        assert!(
            tools.contains(required),
            "Diagnostics must route human-facing copy through i18n: {required}"
        );
    }

    for forbidden in [
        "title=\"No active target\"",
        "text=\"Run a dev server to view diagnostics.\"",
        "['Status', current.status]",
        "['Observer', live.observer.length",
        "'not attached'",
        "['Latest',",
        "['Focused ref',",
        "title=\"Project identity\"",
        "aside=\"diagnostic\"",
        "'Process-derived project identity'",
        "title=\"Runtime pipeline\"",
        "title=\"Semantic refs\"",
        "title=\"Geometry + layout evidence\"",
        "title=\"Source hints\"",
        "title=\"Secure observer drain\"",
        "<em className={state}>{state}</em>",
    ] {
        assert!(
            !tools.contains(forbidden),
            "Diagnostics still leaks hard-coded English/state copy: {forbidden}"
        );
    }
}

#[test]
fn localization_keys_exist_for_every_supported_locale() {
    let i18n = include_str!("../../src/i18n.ts");

    for key in [
        "advanced.status",
        "advanced.notAttached",
        "advanced.latest",
        "advanced.focusedRef",
        "advanced.diagnostic",
        "advanced.processDerivedIdentity",
        "advanced.runtimePipeline",
        "advanced.semanticRefs",
        "advanced.geometryEvidence",
        "advanced.sourceHints",
        "advanced.secureObserverDrain",
        "status.idle",
        "status.ready",
        "status.active",
        "status.disconnected",
        "status.hidden",
        "status.closed",
    ] {
        let needle = format!("'{key}'");
        let count = i18n.matches(&needle).count();
        assert_eq!(
            count, 12,
            "i18n key {key} must exist exactly once in each of the 12 supported locale tables"
        );
    }
}

#[test]
fn browser_audit_proves_vietnamese_diagnostics_without_english_leakage() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    assert!(
        capture.contains("155-vi-advanced-localized.png"),
        "browser audit must retain Vietnamese Diagnostics evidence"
    );

    for marker in [
        "vi-advanced-localized:diagnostics",
        "vi-advanced-localized:no-english-leak",
        "vi-advanced-localized:status",
        "vi-advanced-localized:pipeline-status",
    ] {
        assert!(
            capture.contains(marker),
            "browser audit is missing localization runtime marker: {marker}"
        );
    }
}
