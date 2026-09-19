fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_spec_locks_responsive_authority_and_restore_before_persistence() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-19-trusted-responsive-sweep-contact-sheet-design.md");
    let normalized = spec.to_ascii_lowercase();
    for required in [
        "frontend must never send arbitrary width/height authority",
        "set_min_size(none)",
        "no responsive artifact or evidence may exist before successful preview-size restoration.",
        "responsive_contact_sheet",
        "one session capture gate spans all presets",
        "no workspace/iframe fallback in this slice",
        "restore original preview inner size",
    ] {
        assert!(
            normalized.contains(required),
            "responsive canonical spec missing {required}"
        );
    }
}

#[test]
fn desktop_responsive_request_is_preset_id_only() {
    let api = include_str!("../../src/api.ts");
    assert!(api.contains("ResponsivePresetId"));
    assert!(api.contains("captureResponsiveSweep"));
    assert!(api.contains("'capture_responsive_sweep'"));

    let request = between(api, "ResponsiveSweepRequest", "ResponsiveSweepReceipt");
    for required in ["sessionId", "presets"] {
        assert!(request.contains(required), "responsive request missing {required}");
    }
    for forbidden in [
        "width:", "height:", "viewport:", "deviceScaleFactor", "route:",
        "artifactId", "mask", "pixelWidth", "pixelHeight",
    ] {
        assert!(
            !request.contains(forbidden),
            "frontend responsive request must not author {forbidden}"
        );
    }
}

#[test]
fn responsive_transaction_uses_exact_preview_and_restores_before_persistence() {
    let source = include_str!("../src/visual_capture.rs");
    assert!(source.contains("pub async fn capture_responsive_sweep("));
    let tx = between(
        source,
        "pub async fn capture_responsive_sweep(",
        "pub async fn capture_full_page(",
    );

    for required in [
        "preview_surface_label",
        "DesktopSurfaceKind::PreviewWindow",
        "registry.current",
        "session_capture_gate",
        "set_min_size",
        "set_size",
        "wait_for_capture_settle",
        "freeze_visual_state",
        "capture_managed_surface",
        "restore_visual_state",
        "redact_private_pixels",
        "restore_responsive_preview",
        "build_responsive_contact_sheet",
        "persist_responsive_contact_sheet_and_register",
    ] {
        assert!(tx.contains(required), "responsive transaction missing {required}");
    }

    assert!(
        !tx.contains("workspace_label"),
        "first responsive slice must not silently fall back to workspace/iframe authority"
    );

    let restore = tx.find("restore_responsive_preview").unwrap();
    let persist = tx.find("persist_responsive_contact_sheet_and_register").unwrap();
    assert!(restore < persist, "preview restoration must happen before persistence");

    let restore_fn = between(
        source,
        "async fn restore_responsive_preview(",
        "async fn persist_responsive_contact_sheet_and_register(",
    );
    assert!(
        restore_fn.contains("wait_for_capture_settle"),
        "restored preview must settle before responsive persistence"
    );
    assert!(
        restore_fn.contains("timeout_at(deadline"),
        "restored settle must remain inside the bounded cleanup deadline"
    );

    let loop_start = tx.find("for preset in").expect("responsive preset loop");
    let after_loop = &tx[loop_start..restore];
    assert!(
        !after_loop.contains("artifacts.put("),
        "responsive viewport loop must not persist partial artifacts"
    );
    assert!(
        !after_loop.contains("/evidence/visual-responsive"),
        "responsive viewport loop must not register partial evidence"
    );
}

#[test]
fn responsive_ui_is_real_but_bounded_to_canonical_presets() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let i18n = include_str!("../../src/i18n.ts");

    let responsive = between(tools, "function ResponsivePanel(", "function ConsolePanel(");
    for required in [
        "mobile_s", "mobile", "tablet", "desktop",
        "onRunResponsiveSweep", "responsiveState",
    ] {
        assert!(responsive.contains(required), "Responsive panel missing {required}");
    }
    assert!(!responsive.contains("disabled aria-disabled=\"true\""));
    assert!(!responsive.contains("type=\"number\""));
    assert!(shell.contains("captureResponsiveSweep"));
    assert!(shell.contains("responsiveInFlight"));

    for key in [
        "responsive.run",
        "responsive.inProgress",
        "responsive.success",
        "responsive.failed",
        "responsive.previewRequired",
        "responsive.retry",
    ] {
        assert!(i18n.contains(&format!("'{key}'")), "missing responsive localization {key}");
    }
}

#[test]
fn responsive_evidence_is_dedicated_and_contact_sheet_only() {
    let desktop = include_str!("../src/visual_capture.rs");
    let control = include_str!("../../../../crates/control/src/visual_responsive.rs");

    assert!(desktop.contains("/evidence/visual-responsive"));
    assert!(desktop.contains("ResponsiveVisualEvidenceRequest"));
    assert!(control.contains("responsive_contact_sheet"));
    assert!(control.contains("deny_unknown_fields"));
    assert!(control.contains("ResponsivePresetId"));

    for forbidden in ["freeze_token", "selectors", "cookies", "local_storage", "dom_text"] {
        assert!(
            !control.contains(forbidden),
            "responsive evidence must not retain private authority/content: {forbidden}"
        );
    }
}
