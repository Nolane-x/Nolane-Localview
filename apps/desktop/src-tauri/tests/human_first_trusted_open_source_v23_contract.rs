fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_v23_spec_locks_trusted_source_boundary() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-18-human-first-trusted-open-source-v23.md");

    for required in [
        "session_id",
        "element_reference",
        "React must not submit authoritative:",
        "fresh semantic snapshot",
        "canonicalize the project root",
        "reject symlink escape",
        "No generic filesystem API",
        "Do not merge using green evidence from an earlier head.",
    ] {
        assert!(
            spec.contains(required),
            "canonical Open Source V2.3 spec is missing boundary: {required}"
        );
    }
}

#[test]
fn frontend_source_request_is_reference_only() {
    let api = include_str!("../../src/api.ts");

    assert!(api.contains("openSourceForSelection"));
    assert!(api.contains("'open_source_for_selection'"));
    assert!(api.contains("sessionId"));
    assert!(api.contains("reference"));

    let source_api = between(
        api,
        "openSourceForSelection",
        "measureElement",
    );

    for forbidden in [
        "file:",
        "path:",
        "line:",
        "column:",
        "root:",
        "route:",
        "editor:",
        "command:",
    ] {
        assert!(
            !source_api.contains(forbidden),
            "frontend Open source request must not author {forbidden}"
        );
    }
}

#[test]
fn desktop_resolves_source_from_fresh_session_authority() {
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "open_source_for_selection",
        "validate_source_reference",
        "resolve_trusted_source_target",
        "semantic-snapshot/fresh",
        "canonicalize",
        "canonical_project_root",
        "canonical_file",
        "project_relative_file",
        "validate_source_line_exists",
        "MAX_SOURCE_VERIFY_BYTES",
    ] {
        assert!(
            desktop.contains(required),
            "desktop trusted source authority is missing {required}"
        );
    }

    assert!(
        desktop.contains("managed_surface_canonical_route"),
        "source resolution must bind to exact managed route"
    );
    assert!(
        desktop.contains("pre_route") && desktop.contains("post_route"),
        "source resolution must reject route drift"
    );
}

#[test]
fn desktop_source_launcher_is_bounded_and_not_shell_authored() {
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "TrustedSourceTarget",
        "SourceOpenLauncher",
        "trusted_source_launch_plan",
        "launch_trusted_source_with",
        "launch_trusted_source",
        "source outside project",
    ] {
        assert!(
            desktop.contains(required),
            "trusted source launcher is missing {required}"
        );
    }

    for forbidden in [
        "sh -c",
        "cmd /C",
        "powershell -Command",
        "open_path_from_frontend",
    ] {
        assert!(
            !desktop.contains(forbidden),
            "trusted source launcher must not expose shell/file authority: {forbidden}"
        );
    }
}

#[test]
fn inspector_open_source_is_real_but_never_uses_focus_payload_as_authority() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    let inspector = between(tools, "function Inspector(", "function UnavailableInspectorAction(");

    assert!(inspector.contains("className=\"source-open-action\""));
    assert!(inspector.contains("onOpenSource"));
    assert!(inspector.contains("sourceOpenState"));
    assert!(
        !inspector.contains("onOpenSource(source)"),
        "Inspector must not forward focus payload source text as target authority"
    );

    assert!(shell.contains("HumanSourceOpenState"));
    assert!(shell.contains("openSourceForSelection"));
    assert!(shell.contains("reference: sourceOpenReference"));
    assert!(shell.contains("setSourceOpenState"));
}

#[test]
fn source_open_failure_is_humanized_and_localized() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let i18n = include_str!("../../src/i18n.ts");

    for key in [
        "source.opening",
        "source.opened",
        "source.unavailable",
        "source.failed",
        "source.launcherUnavailable",
        "source.selectFirst",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing trusted source localization key {key}"
        );
    }

    assert!(tools.contains("translate(locale, 'source.opening')"));
    assert!(tools.contains("translate(locale, 'source.opened')"));
    assert!(tools.contains("translate(locale, 'source.failed')"));
    assert!(
        !tools.contains("element reference not found"),
        "default Inspector must not leak raw source resolution errors"
    );
}

#[test]
fn render_audit_proves_trusted_open_source_runtime_states() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "46-source-open-success.png",
        "47-source-open-failure.png",
        "48-source-open-unavailable.png",
        "49-source-open-stale-selection.png",
        "50-source-open-outside-project.png",
        "51-vi-source-open-success.png",
        "52-source-open-no-selection.png",
        "53-source-open-no-session.png",
        "54-source-open-malformed-reference.png",
        "55-source-open-path-traversal.png",
        "56-source-open-symlink-escape.png",
        "57-source-open-launcher-failure.png",
        "58-source-open-failure-isolation.png",
        "59-vi-source-open-failure.png",
    ] {
        assert!(
            capture.contains(artifact),
            "trusted source render audit is missing {artifact}"
        );
    }

    for marker in [
        "open_source_for_selection",
        "sourceOpenCalls",
        "source-open:no-raw-error",
        "source-open:request-reference-only",
        "source-open:no-caller-path-authority",
        "source-open:stale-selection-isolated",
        "source-open:path-traversal",
        "source-open:symlink-escape",
        "source-open:launcher-no-raw-error",
        "source-open:no-selection-not-invoked",
        "source-open:no-session-not-invoked",
        "source-open:malformed-reference-not-invoked",
    ] {
        assert!(
            capture.contains(marker),
            "trusted source render audit is missing executable marker {marker}"
        );
    }
}
