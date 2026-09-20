fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_design_locks_affected_region_truth_boundary() {
    let spec = include_str!(
        "../../../../docs/superpowers/specs/2026-09-20-wave5-affected-region-trusted-verify-design.md"
    );

    for required in [
        "affected-region recapture",
        "resolve_progressive_targets",
        "120 CSS-pixel",
        "Crop before redaction is forbidden",
        "capture_region: Option<Rect>",
        "affectedRegionChangedRatio",
        "verificationId",
        "No new durable full-viewport baseline",
        "exact stored backend-owned capture region",
    ] {
        assert!(
            spec.contains(required),
            "affected-region Verify design is missing boundary: {required}"
        );
    }
}

#[test]
fn apply_derives_visual_region_from_progressive_element_before_source_write() {
    let desktop = include_str!("../src/lib.rs");
    let apply = between(
        desktop,
        "async fn apply_fix_proposal(",
        "fn discard_fix_proposal(",
    );

    for required in [
        "localview_capture::resolve_progressive_targets(",
        "localview_capture::ProgressiveTargetKind::Element",
        "capture_verification_baseline(",
        "verification_region",
        "capture_region: frame.region",
        "apply_fix_transaction(",
    ] {
        assert!(
            apply.contains(required),
            "trusted Apply affected-region path is missing {required}"
        );
    }

    let resolve = apply
        .find("localview_capture::resolve_progressive_targets(")
        .expect("progressive target resolution");
    let baseline = apply
        .find("capture_verification_baseline(")
        .expect("visual baseline");
    let write = apply
        .find("apply_fix_transaction(")
        .expect("source write");
    assert!(
        resolve < baseline && baseline < write,
        "backend target resolution and baseline capture must happen before source mutation"
    );
}

#[test]
fn visual_region_crop_remains_after_restore_and_private_redaction() {
    let visual = include_str!("../src/visual_capture.rs");
    let capture = between(
        visual,
        "async fn capture_current_redacted_frame_with_freeze(",
        "async fn capture_current_redacted_frame(",
    );

    let restore = capture
        .find("restore_visual_state(")
        .expect("restore acknowledgement");
    let redact = capture
        .find("redact_private_pixels(frame, &freeze)")
        .expect("private redaction");
    assert!(
        restore < redact,
        "private redaction may only happen after restore acknowledgement"
    );

    let target = between(
        visual,
        "async fn capture_verification_target_frame(",
        "fn verification_visual_frame(",
    );
    for required in [
        "capture_current_redacted_frame_with_freeze(",
        "RequestedCaptureTarget::Region",
        "validate_live_target_viewport(",
        "apply_capture_target(",
    ] {
        assert!(
            target.contains(required),
            "verification region capture is missing {required}"
        );
    }

    let redacted_source = target
        .find("capture_current_redacted_frame_with_freeze(")
        .expect("already-redacted source");
    let crop = target
        .find("apply_capture_target(")
        .expect("region crop");
    assert!(
        redacted_source < crop,
        "affected region must be cropped only from the restored/redacted frame"
    );
}

#[test]
fn verify_recaptures_only_backend_stored_region_and_registers_region_evidence() {
    let desktop = include_str!("../src/lib.rs");
    let verify = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );

    for required in [
        "wait_for_verification_settle(record.session_id)",
        "before.capture_region.clone()",
        "frame.region != before.capture_region",
        "affected_region_changed_ratio",
        "register_verification_visual_diff_evidence(",
        "frame.region.clone()",
    ] {
        assert!(
            verify.contains(required),
            "trusted Verify affected-region loop is missing {required}"
        );
    }

    for forbidden in [
        "verification_region:",
        "region: request",
        "region: payload",
        "frontend_region",
    ] {
        assert!(
            !verify.contains(forbidden),
            "Verify must not accept new region authority from the frontend: {forbidden}"
        );
    }
}

#[test]
fn region_visual_evidence_and_ratios_are_semantically_distinct() {
    let visual = include_str!("../src/visual_capture.rs");
    let verify = include_str!("../src/trusted_verify.rs");
    let api = include_str!("../../src/api.ts");

    for required in [
        "region: Option<Rect>",
        "target.region()",
        "("region", vec![current_visual_evidence_id])",
    ] {
        assert!(
            visual.contains(required),
            "visual region evidence is missing {required}"
        );
    }

    for required in [
        "capture_region: Option<Rect>",
        "affected_region_changed_ratio",
        "trusted Verify affected-region diff failed",
        "contains_rect(capture_region, before_rect)",
        "contains_rect(capture_region, after_rect)",
        "affected_region_visual_changed",
    ] {
        assert!(
            verify.contains(required),
            "deterministic region verification is missing {required}"
        );
    }

    assert!(
        api.contains("affectedRegionChangedRatio?: number | null"),
        "frontend receipt type must expose additive affected-region evidence"
    );
}

#[test]
fn legacy_trusted_verify_authority_remains_verification_id_only() {
    let api = include_str!("../../src/api.ts");
    let request = between(
        api,
        "export interface HumanVerifyChangeRequest",
        "export interface HumanVerifyChangeReceipt",
    );

    assert!(request.contains("verificationId: string"));
    for forbidden in [
        "sessionId",
        "reference",
        "region",
        "rect",
        "viewport",
        "baseline",
        "source",
    ] {
        assert!(
            !request.contains(forbidden),
            "affected-region work must not expand frontend Verify authority with {forbidden}"
        );
    }
}
