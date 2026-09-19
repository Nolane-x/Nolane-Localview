fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_spec_locks_primary_localization_integrity() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-18-human-first-localview-ui-ux-v2.md");

    for required in [
        "No split-language primary flow",
        "every supported locale is registered",
        "every canonical primary-flow key has an English fallback",
        "locale normalization is deterministic",
        "stored invalid locale cannot corrupt startup",
        "Prefer programmatic key-set verification over string-presence-only checks",
        "Audit human-facing strings in touched V2 surfaces",
    ] {
        assert!(
            spec.contains(required),
            "Human-First V2 localization contract is missing: {required}"
        );
    }
}

#[test]
fn locale_module_exposes_programmatic_primary_flow_integrity() {
    let i18n = include_str!("../../src/i18n.ts");

    for required in [
        "PRIMARY_FLOW_MESSAGE_KEYS",
        "localeIntegrityReport",
        "missingPrimaryKeys",
        "missingEnglishFallbackKeys",
        "emptyPrimaryKeys",
        "Record<SupportedLocale, Dictionary>",
    ] {
        assert!(
            i18n.contains(required),
            "locale integrity implementation is missing {required}"
        );
    }

    for key in [
        "responsive.mobileSmall",
        "responsive.mobile",
        "responsive.tablet",
        "responsive.desktop",
        "session.status.active",
        "session.status.disconnected",
        "session.status.hidden",
        "session.framework.web",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "locale vocabulary is missing primary-flow key {key}"
        );
    }
}

#[test]
fn responsive_primary_flow_uses_translation_keys_instead_of_english_labels() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let responsive = between(tools, "function ResponsivePanel(", "function ConsolePanel(");

    for key in [
        "responsive.mobileSmall",
        "responsive.mobile",
        "responsive.tablet",
        "responsive.desktop",
    ] {
        assert!(
            responsive.contains(key),
            "Responsive primary flow is missing localized key {key}"
        );
    }

    for forbidden in [
        "['Mobile S', '320', '568']",
        "['Mobile', '390', '844']",
        "['Tablet', '768', '1024']",
        "['Desktop', '1440', '900']",
    ] {
        assert!(
            !responsive.contains(forbidden),
            "Responsive primary flow still contains hard-coded English label {forbidden}"
        );
    }
}

#[test]
fn sessions_primary_flow_localizes_status_and_default_framework_label() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let sessions = between(tools, "function SessionsPanel(", "function CommandPanel(");

    for required in [
        "session.status.active",
        "session.status.disconnected",
        "session.status.hidden",
        "session.framework.web",
        "translate(locale",
    ] {
        assert!(
            sessions.contains(required),
            "Sessions primary flow is missing localized mapping {required}"
        );
    }

    assert!(
        !sessions.contains("session.status}</span>"),
        "Sessions panel must not render the raw session status enum"
    );
    assert!(
        !sessions.contains("?? 'Web'"),
        "Sessions panel must not hard-code the English Web fallback"
    );
}

#[test]
fn programmatic_locale_audit_executes_real_key_sets() {
    let audit = include_str!("../../../../tools/human-first-ui-v2/check-locales.mjs");

    for required in [
        "typescript",
        "transpileModule",
        "PRIMARY_FLOW_MESSAGE_KEYS",
        "SUPPORTED_LOCALES",
        "localeIntegrityReport",
        "missingPrimaryKeys",
        "missingEnglishFallbackKeys",
        "emptyPrimaryKeys",
    ] {
        assert!(
            audit.contains(required),
            "programmatic localization audit is missing {required}"
        );
    }
}

#[test]
fn browser_audit_proves_no_split_language_for_responsive_and_sessions() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "150-vi-responsive-localized.png",
        "151-vi-sessions-localized.png",
    ] {
        assert!(
            capture.contains(artifact),
            "localization browser audit is missing {artifact}"
        );
    }

    for marker in [
        "localization:vi-responsive-presets",
        "localization:vi-sessions-status",
        "localization:vi-session-web-fallback",
        "localization:no-primary-english-leak",
    ] {
        assert!(
            capture.contains(marker),
            "localization browser audit is missing marker {marker}"
        );
    }
}
