#[test]
fn trusted_capture_command_is_desktop_owned_and_geometry_free_at_the_boundary() {
    let capture = include_str!("../src/visual_capture.rs");
    let lib = include_str!("../src/lib.rs");

    assert!(
        capture.contains("pub async fn capture_current_viewport("),
        "V2.1 requires a dedicated trusted current-viewport command"
    );
    let command = capture
        .split("pub async fn capture_current_viewport(")
        .nth(1)
        .expect("trusted capture command must exist")
        .split(") -> Result<VisualCaptureReceipt, String>")
        .next()
        .expect("trusted capture command signature must be bounded");

    assert!(command.contains("session_id: SessionId"));
    assert!(command.contains("revision: Option<String>"));
    assert!(
        !command.contains("viewport: ViewportMeta"),
        "React/caller must not author authoritative viewport geometry"
    );

    assert!(capture.contains("managed_surface_scale_factor("));
    assert!(capture.contains("trusted_viewport_from_freeze("));
    assert!(capture.contains("viewport_css_width"));
    assert!(capture.contains("viewport_css_height"));
    assert!(capture.contains("scale_factor"));
    assert!(lib.contains("visual_capture::capture_current_viewport"));
}

#[test]
fn trusted_capture_geometry_conversion_is_explicit_and_fail_closed() {
    let capture = include_str!("../src/visual_capture.rs");

    for marker in [
        "TRUSTED_VIEWPORT_INTEGRAL_TOLERANCE_CSS",
        "trusted_css_dimension(",
        "is_finite()",
        "round()",
        "u32::MAX",
        "device scale factor",
    ] {
        assert!(
            capture.contains(marker),
            "trusted capture is missing geometry safety marker {marker}"
        );
    }
}

#[test]
fn frontend_capture_api_never_sends_viewport_geometry() {
    let api = include_str!("../../src/api.ts");

    assert!(api.contains("captureCurrentViewport"));
    assert!(api.contains("capture_current_viewport"));
    let method = api
        .split("captureCurrentViewport")
        .nth(1)
        .expect("capture API must exist")
        .split("\n")
        .next()
        .expect("capture API line must exist");

    assert!(method.contains("sessionId"));
    assert!(
        !method.contains("viewport"),
        "human Capture API must not accept caller viewport metadata"
    );
}

#[test]
fn inspector_capture_is_real_but_sibling_unwired_actions_remain_fail_closed() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    let inspector = tools
        .split("function Inspector(")
        .nth(1)
        .expect("Inspector must exist")
        .split("function AdvancedPanel(")
        .next()
        .expect("Inspector boundary must exist");

    assert!(inspector.contains("onCapture"));
    assert!(inspector.contains("captureState"));
    assert!(inspector.contains("CaptureIcon"));
    assert!(inspector.contains("disabled={!current"));
    assert!(
        !inspector.contains("UnavailableInspectorAction icon={<CaptureIcon"),
        "Capture must leave the unavailable-action path once trusted wiring exists"
    );

    for action in ["SourceIcon", "RulerIcon", "SparkIcon", "ActivityIcon"] {
        assert!(
            inspector.contains(action),
            "sibling fail-closed action marker missing: {action}"
        );
    }
}

#[test]
fn shell_owns_human_capture_state_and_isolates_failures() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    assert!(shell.contains("captureState"));
    assert!(shell.contains("captureCurrentViewport"));
    assert!(shell.contains("setCaptureState"));
    assert!(shell.contains("try {"));
    assert!(shell.contains("catch"));
    assert!(shell.contains("onCapture="));
}

#[test]
fn capture_human_copy_is_localized() {
    let i18n = include_str!("../../src/i18n.ts");

    for key in [
        "capture.inProgress",
        "capture.success",
        "capture.failed",
        "capture.unavailable",
        "capture.evidence",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing trusted Capture localization key {key}"
        );
    }

    assert!(i18n.contains("type Dictionary = Record<MessageKey, string>;"));
}

#[test]
fn trusted_capture_runtime_audit_is_executable_and_geometry_free() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "31-trusted-capture-success.png",
        "32-trusted-capture-failure.png",
        "33-vi-trusted-capture-success.png",
        "34-no-target-capture-disabled.png",
        "35-trusted-capture-in-progress.png",
    ] {
        assert!(
            capture.contains(artifact),
            "trusted Capture runtime audit is missing {artifact}"
        );
    }

    for marker in [
        "__LOCALVIEW_AUDIT_INVOKES__",
        "capture_current_viewport",
        "no-caller-viewport",
        "single-request",
        "no-raw-error",
    ] {
        assert!(
            capture.contains(marker),
            "trusted Capture runtime audit is missing invariant marker {marker}"
        );
    }
}

#[test]
fn trusted_capture_shell_invalidates_stale_session_results_and_guards_duplicates() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    for marker in [
        "useRef",
        "captureInFlight",
        "captureGeneration",
        "captureGeneration.current += 1",
        "captureInFlight.current = false",
        "generation !== captureGeneration.current",
    ] {
        assert!(
            shell.contains(marker),
            "trusted Capture lifecycle is missing guard marker {marker}"
        );
    }
}

#[test]
fn trusted_capture_command_is_allowed_by_main_dashboard_permission() {
    let permissions = include_str!("../permissions/localview.toml");
    let main = permissions
        .split("identifier = \"maincommands\"")
        .nth(1)
        .expect("main dashboard permission must exist")
        .split("[[permission]]")
        .next()
        .expect("main dashboard permission must be bounded");

    assert!(
        main.contains("\"capture_current_viewport\""),
        "trusted Capture command must be callable by the bundled dashboard"
    );
}
