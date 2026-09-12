#[test]
fn changed_region_transaction_publishes_diff_before_advancing_baseline() {
    let module = include_str!("../src/visual_capture.rs");
    let changed_start = module
        .find("pub async fn capture_changed_regions(")
        .expect("changed-region command must exist");
    let changed_end = module[changed_start..]
        .find("\nasync fn capture_redacted_viewport_after_gate(")
        .map(|offset| changed_start + offset)
        .expect("changed-region command boundary must exist");
    let changed = &module[changed_start..changed_end];

    assert!(changed.contains("register_visual_diff_evidence("));
    assert!(changed.contains("visual_diff_evidence_id"));

    let emit = changed
        .find("emit_changed_capture_plan(")
        .expect("changed visual transaction must emit selected visual evidence first");
    let diff = changed
        .rfind("register_visual_diff_evidence(")
        .expect("changed visual transaction must publish retained diff evidence");
    let baseline = changed
        .find("commit_changed_baseline(")
        .expect("changed visual transaction must advance the baseline");

    assert!(emit < diff, "diff provenance must point at emitted visual evidence");
    assert!(diff < baseline, "baseline must not advance before diff evidence is retained");
}

#[test]
fn unchanged_observation_is_still_retained_without_visual_parents() {
    let module = include_str!("../src/visual_capture.rs");
    let changed_start = module
        .find("pub async fn capture_changed_regions(")
        .expect("changed-region command must exist");
    let changed_end = module[changed_start..]
        .find("\nasync fn capture_redacted_viewport_after_gate(")
        .map(|offset| changed_start + offset)
        .expect("changed-region command boundary must exist");
    let changed = &module[changed_start..changed_end];

    let unchanged = changed
        .find("ChangedRegionPlan::Unchanged")
        .expect("unchanged branch must exist");
    let tail = &changed[unchanged..];
    assert!(tail.contains("register_visual_diff_evidence("));
    assert!(tail.contains("Vec::new()"));
    assert!(tail.contains("changed_ratio: 0.0"));
}

#[test]
fn visual_diff_publisher_sends_only_bounded_metadata_and_parent_evidence_ids() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("struct VisualDiffEvidenceRequest"));
    assert!(module.contains("visual_evidence_ids: Vec<String>"));
    assert!(module.contains("/evidence/visual-diff"));
    assert!(module.contains("struct VisualDiffEvidenceResponse"));

    let request_start = module
        .find("struct VisualDiffEvidenceRequest")
        .expect("visual diff request must exist");
    let response_start = module[request_start..]
        .find("struct VisualDiffEvidenceResponse")
        .map(|offset| request_start + offset)
        .expect("visual diff response must follow request");
    let request = &module[request_start..response_start];

    assert!(request.contains("route: String"));
    assert!(request.contains("viewport: ViewportMeta"));
    assert!(request.contains("revision: Option<String>"));
    assert!(request.contains("captured_at_unix_ms: i64"));
    assert!(request.contains("mode: &'static str"));
    assert!(request.contains("changed_ratio: f64"));
    assert!(!request.contains("png"));
    assert!(!request.contains("artifact_id"));
    assert!(!request.contains("verdict"));
}
