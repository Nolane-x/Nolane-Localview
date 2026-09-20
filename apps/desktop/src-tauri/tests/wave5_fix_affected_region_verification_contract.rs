fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn trusted_verify_uses_settle_fresh_snapshot_and_one_post_apply_capture() {
    let desktop = include_str!("../src/lib.rs");
    let verify = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );

    let settle = verify
        .find("wait_for_verification_settle")
        .expect("Verify must use existing settle authority");
    let fresh = verify
        .find("semantic-snapshot/fresh")
        .expect("Verify must acquire an exact fresh semantic snapshot");
    let capture = verify
        .find("capture_verification_current(")
        .expect("Verify must acquire one redacted current visual frame");
    let assess = verify
        .find("assess_visual_change(")
        .expect("Verify must plan the affected visual change");
    let persist = verify
        .find("persist_verification_affected_visual_evidence(")
        .expect("Verify must persist the bounded affected evidence");

    assert!(settle < fresh);
    assert!(fresh < capture);
    assert!(capture < assess);
    assert!(assess < persist);
    assert_eq!(verify.matches("capture_verification_current(").count(), 1);
    assert!(!verify.contains("capture_changed_regions("));
    assert!(!verify.contains("capture_registered_verification_current("));
    assert!(!verify.contains("capture_current_viewport("));
}

#[test]
fn affected_region_planner_reuses_verify_threshold_and_existing_visual_planner() {
    let verify = include_str!("../src/trusted_verify.rs");

    for required in [
        "assess_visual_change",
        "ChangedRegionPolicy",
        "threshold: VERIFY_PIXEL_THRESHOLD",
        "plan_changed_css_regions",
        "ChangedRegionPlan::Unchanged",
        "ChangedRegionPlan::Regions",
        "ChangedRegionPlan::Viewport",
        "VerificationVisualChangeMode::Unchanged",
        "VerificationVisualChangeMode::Regions",
        "VerificationVisualChangeMode::Viewport",
    ] {
        assert!(
            verify.contains(required),
            "affected-region Verify planner is missing {required}"
        );
    }

    assert!(
        verify.contains("pixel_diff(&before_image, &after_image, VERIFY_PIXEL_THRESHOLD)"),
        "viewport deterministic diff must keep the same threshold"
    );
}

#[test]
fn affected_visual_evidence_persists_crops_without_second_capture() {
    let visual = include_str!("../src/visual_capture.rs");
    let helper = between(
        visual,
        "pub(crate) async fn persist_verification_affected_visual_evidence(",
        "#[tauri::command]\npub async fn capture_current_viewport(",
    );

    for required in [
        ""unchanged" =>",
        ""regions" =>",
        ""viewport" =>",
        "decode_png_rgba(&frame.png)",
        "crop_css_rect(",
        "encode_png_rgba(&cropped)",
        "RequestedCaptureTarget::Region",
        "RequestedCaptureTarget::Viewport",
        "persist_and_register(",
        "register_visual_diff_evidence(",
        "visual_evidence_ids.clone()",
    ] {
        assert!(
            helper.contains(required),
            "affected visual persistence is missing {required}"
        );
    }

    for forbidden in [
        "capture_managed_surface(",
        "capture_current_redacted_frame(",
        "freeze_visual_state(",
        "wait_for_capture_settle(",
    ] {
        assert!(
            !helper.contains(forbidden),
            "affected evidence persistence must not acquire pixels again: {forbidden}"
        );
    }

    let unchanged = between(helper, ""unchanged" =>", ""regions" =>");
    assert!(!unchanged.contains("persist_and_register("));
    assert!(!unchanged.contains("encode_png_rgba("));
}

#[test]
fn affected_region_receipt_is_bounded_and_backend_authored() {
    let verify = include_str!("../src/trusted_verify.rs");
    let api = include_str!("../../src/api.ts");
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "visual_change_mode",
        "affected_regions",
        "affected_visual_evidence_ids",
        "visual_diff_evidence_id",
    ] {
        assert!(
            verify.contains(required) || desktop.contains(required),
            "Rust Verify receipt is missing {required}"
        );
    }

    for required in [
        "visualChangeMode",
        "affectedRegions",
        "affectedVisualEvidenceIds",
        "VerifyVisualChangeMode",
    ] {
        assert!(
            api.contains(required),
            "frontend receipt contract is missing {required}"
        );
    }

    let verify_command = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );
    for forbidden in [
        "affected_regions:",
        "visual_change_mode:",
        "affectedVisualEvidenceIds:",
    ] {
        assert!(
            !verify_command
                .split("let mut visual_change_mode = None;")
                .next()
                .unwrap_or_default()
                .contains(forbidden),
            "caller must not author affected-region evidence: {forbidden}"
        );
    }
}

#[test]
fn affected_region_verification_preserves_deterministic_status_boundary() {
    let verify = include_str!("../src/trusted_verify.rs");
    let classifier = between(
        verify,
        "pub fn classify_verification_status(",
        "pub fn mint_verification_baseline(",
    );

    for required in [
        "RegressionSignal",
        "ChangeObserved",
        "Inconclusive",
        "NoObservableChange",
        "target_changed_ratio",
        "viewport_changed_ratio",
    ] {
        assert!(
            classifier.contains(required),
            "deterministic Verify classifier lost {required}"
        );
    }

    assert!(!classifier.contains("affected_regions"));
    assert!(!classifier.contains("VerificationVisualChangeMode"));
}
