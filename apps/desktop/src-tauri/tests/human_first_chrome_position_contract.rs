#[test]
fn canonical_human_first_spec_requires_bounded_chrome_position_restore() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-18-human-first-localview-ui-ux-v2.md");

    for required in [
        "coordinates must be finite",
        "restored chrome must be clamped to the current usable window",
        "monitor/resolution change must not strand controls off-screen",
        "resetting workspace clears saved chrome positions",
        "drag handles must be keyboard/accessibility compatible",
        "Do not blindly trust stale persisted coordinates",
    ] {
        assert!(
            spec.contains(required),
            "Human-First V2 chrome position contract is missing: {required}"
        );
    }
}

#[test]
fn preference_layer_exposes_finite_clamp_and_workspace_reset_boundaries() {
    let preferences = include_str!("../../src/preferences.ts");

    for required in [
        "safePoint",
        "Number.isFinite",
        "clampChromePoint",
        "CHROME_EDGE_MARGIN",
        "targetBarPosition: null",
        "toolRailPosition: null",
    ] {
        assert!(
            preferences.contains(required),
            "preference chrome safety is missing {required}"
        );
    }
}

#[test]
fn shell_wires_explicit_pointer_and_keyboard_reposition_without_page_authority() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    for required in [
        "useMovableChrome",
        "setPointerCapture",
        "onPointerDown",
        "onKeyDown",
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "Home",
        "targetBarPosition",
        "toolRailPosition",
        "rememberChromePositions",
        "aria.moveTargetBar",
        "aria.moveToolRail",
    ] {
        assert!(
            shell.contains(required),
            "movable chrome runtime is missing {required}"
        );
    }

    for forbidden in [
        "document.body.style.transform",
        "document.documentElement.style.transform",
        "workspace.style.transform",
        "app-frame.style.transform",
    ] {
        assert!(
            !shell.contains(forbidden),
            "chrome reposition must not alter inspected page layout: {forbidden}"
        );
    }
}

#[test]
fn settings_exposes_remember_positions_and_localized_accessible_handles() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let i18n = include_str!("../../src/i18n.ts");
    let styles = include_str!("../../src/styles.css");

    assert!(
        tools.contains("rememberChromePositions"),
        "Settings must expose the existing rememberChromePositions preference"
    );
    assert!(i18n.contains("'aria.moveTargetBar'"));
    assert!(i18n.contains("'aria.moveToolRail'"));
    assert!(styles.contains(".chrome-drag-handle"));
    assert!(styles.contains("touch-action:none"));
}

#[test]
fn render_audit_proves_restore_drag_keyboard_resize_and_reset() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "146-stale-chrome-clamped.png",
        "147-chrome-drag-persisted.png",
        "148-chrome-keyboard-move.png",
        "149-chrome-resize-reset-recovered.png",
    ] {
        assert!(
            capture.contains(artifact),
            "chrome position runtime audit is missing {artifact}"
        );
    }

    for marker in [
        "chrome:restored-clamped",
        "chrome:drag-persisted",
        "chrome:keyboard-move-persisted",
        "chrome:remember-disabled-ephemeral",
        "chrome:resize-clamped",
        "chrome:reset-clears-positions",
    ] {
        assert!(
            capture.contains(marker),
            "chrome position runtime audit is missing marker {marker}"
        );
    }
}
