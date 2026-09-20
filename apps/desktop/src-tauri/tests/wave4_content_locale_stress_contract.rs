#[test]
fn desktop_registers_bounded_content_stress_authority() {
    let desktop = include_str!("../src/lib.rs");
    let module = include_str!("../src/content_stress.rs");
    let api = include_str!("../../src/api.ts");
    let permissions = include_str!("../permissions/localview.toml");

    for required in [
        "mod content_stress;",
        "ContentStressState::default()",
        "capture_content_locale_stress",
        "preview_complete_content_stress",
        "takeContentStressCompletions",
    ] {
        assert!(desktop.contains(required), "missing desktop wiring: {required}");
    }

    for required in [
        "Expanded130",
        "Expanded180",
        "DenseCjk",
        "RtlPseudo",
        "content_viewport_overflow",
        "content_sibling_collision",
        "content_interactive_disappeared",
        "content_stress_restore_validation_failed",
        "wait_for_content_stress_settle",
        "fresh_semantic_snapshot",
        "session_capture_gate",
        "_capture_guard",
        "best_effort_restore",
    ] {
        assert!(module.contains(required), "missing content-stress authority: {required}");
    }

    assert!(api.contains("captureContentLocaleStress"));
    assert!(api.contains("ContentStressReceipt"));
    assert!(permissions.contains("\"capture_content_locale_stress\""));
    assert!(permissions.contains("\"preview_complete_content_stress\""));
}

#[test]
fn content_stress_receipt_is_synthetic_and_does_not_claim_translation_authority() {
    let module = include_str!("../src/content_stress.rs");
    for forbidden in [
        "translated_text",
        "translation_quality",
        "real_translation",
        "locale_translation",
        "source_text:",
    ] {
        assert!(!module.contains(forbidden), "forbidden authority claim: {forbidden}");
    }
    assert!(module.contains("pub synthetic: bool"));
    assert!(module.contains("synthetic: true"));
}

#[test]
fn stress_restore_conflicts_fail_closed_instead_of_overwriting_app_state() {
    let instrumentation = include_str!("../../../../crates/instrumentation/src/lib.rs");
    assert!(instrumentation.contains("node.nodeValue === entry.stressed"));
    assert!(instrumentation.contains("node.nodeValue === entry.original"));
    assert!(instrumentation.contains("conflictNodes += 1"));
    assert!(instrumentation.contains("restore_conflict"));
}
