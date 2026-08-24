#[test]
fn changed_region_capture_is_registered_bounded_and_shares_the_redacted_native_transaction() {
    let lib = include_str!("../src/lib.rs");
    let module = include_str!("../src/visual_capture.rs");

    assert!(lib.contains("visual_capture::capture_changed_regions"));
    assert!(module.contains("VISUAL_BASELINE_BUDGET_BYTES"));
    assert!(module.contains("MAX_VISUAL_BASELINES"));
    assert!(module.contains("baselines: Mutex<Option<VisualBaselineCache>>"));

    let helper_start = module
        .find("async fn capture_redacted_viewport_after_gate(")
        .expect("shared redacted viewport helper must exist");
    let helper_end = module[helper_start..]
        .find("\nasync fn ")
        .map(|offset| helper_start + offset)
        .unwrap_or(module.len());
    let helper = &module[helper_start..helper_end];

    let settle = helper
        .find("wait_for_capture_settle(session_id).await?")
        .expect("shared helper must settle before capture");
    let freeze = helper
        .find("freeze_visual_state(session_id).await?")
        .expect("shared helper must freeze visual motion");
    let pixels = helper
        .find("capture_managed_surface(&app, session_id, viewport, revision).await")
        .expect("shared helper must perform one native viewport acquisition");
    let restore = helper
        .find("restore_visual_state(session_id, &freeze.token).await")
        .expect("shared helper must restore the exact freeze token");
    let redact = helper
        .find("redact_private_pixels(frame, &freeze)?")
        .expect("shared helper must redact before returning pixels");

    assert_eq!(helper.matches("capture_managed_surface(").count(), 1);
    assert!(settle < freeze);
    assert!(freeze < pixels);
    assert!(pixels < restore);
    assert!(restore < redact);

    let target_start = module
        .find("async fn capture_target(")
        .expect("ordinary capture target transaction must exist");
    let target_end = module[target_start..]
        .find("\nasync fn ")
        .map(|offset| target_start + offset)
        .unwrap_or(module.len());
    let target = &module[target_start..target_end];
    assert!(target.contains("capture_redacted_viewport_after_gate("));

    let changed_start = module
        .find("pub async fn capture_changed_regions(")
        .expect("changed-region command must exist");
    let changed_end = module[changed_start..]
        .find("\nasync fn ")
        .map(|offset| changed_start + offset)
        .unwrap_or(module.len());
    let changed = &module[changed_start..changed_end];
    assert_eq!(
        changed.matches("capture_redacted_viewport_after_gate(").count(),
        1,
        "changed scheduling must acquire native pixels once per transaction"
    );
}

#[test]
fn changed_region_baseline_is_private_redacted_and_advances_only_after_evidence_success() {
    let module = include_str!("../src/visual_capture.rs");
    let changed_start = module
        .find("pub async fn capture_changed_regions(")
        .expect("changed-region command must exist");
    let changed_end = module[changed_start..]
        .find("\nasync fn ")
        .map(|offset| changed_start + offset)
        .unwrap_or(module.len());
    let changed = &module[changed_start..changed_end];

    let acquire = changed
        .find("capture_redacted_viewport_after_gate(")
        .expect("changed capture must receive only redacted viewport pixels");
    let decode = changed
        .find("decode_png_rgba(&frame.png)")
        .expect("changed capture must decode the redacted viewport exactly once");
    let baseline = changed
        .find("compatible_changed_baseline(")
        .expect("changed capture must load a compatible bounded baseline");
    let plan = changed
        .find("plan_changed_css_regions(")
        .expect("changed capture must run the deterministic planner");
    let emit = changed
        .find("emit_changed_capture_plan(")
        .expect("changed capture must emit the selected viewport/region evidence");
    let commit = changed
        .find("commit_changed_baseline(")
        .expect("changed capture must advance the baseline only after emission");

    assert!(acquire < decode);
    assert!(decode < baseline);
    assert!(baseline < plan);
    assert!(plan < emit);
    assert!(emit < commit);
    assert!(changed.contains("ChangedRegionPlan::Unchanged"));
    assert!(changed.contains("mode: \"unchanged\""));
    assert!(changed.contains("baseline_cached: true"));
}

#[test]
fn changed_region_emission_reuses_one_decoded_frame_for_all_region_crops() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("RgbaImage"));
    assert!(module.contains("crop_css_rect"));
    assert!(module.contains("encode_png_rgba"));
    assert!(module.contains("ChangedRegionPlan::Regions"));
    assert!(module.contains("RequestedCaptureTarget::Region(rect.clone())"));
    assert!(module.contains("ChangedRegionPlan::Viewport"));
    assert!(module.contains("mode: \"baseline_reset\""));
    assert!(module.contains("mode: \"regions\""));
    assert!(module.contains("mode: \"viewport\""));
}