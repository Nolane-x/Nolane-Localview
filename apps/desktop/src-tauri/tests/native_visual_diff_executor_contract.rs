#[test]
fn native_executor_visual_diff_action_reuses_changed_region_transaction() {
    let worker = include_str!("../src/native_executor_worker.rs");
    let packet = include_str!("../src/visual_packet_impl.rs");
    let shared = include_str!("../src/visual_capture.rs");

    assert!(worker.contains("execute_native_visual_packet"));
    assert!(packet.contains("NativeExecutorAction::VisualDiffCapture"));
    assert!(packet.contains("capture_changed_regions("));
    assert!(packet.contains("visual_diff_evidence_id"));
    assert!(packet.contains("evidence_ids"));
    assert!(shared.contains("register_visual_diff_evidence("));

    let execute_start = packet
        .find("pub(crate) async fn execute_native_visual_packet(")
        .expect("native visual executor must exist");
    let execute = &packet[execute_start..];
    let diff_action = execute
        .find("NativeExecutorAction::VisualDiffCapture")
        .expect("native executor must support visual diff capture");
    let changed_capture = execute[diff_action..]
        .find("capture_changed_regions(")
        .map(|offset| diff_action + offset)
        .expect("visual diff action must reuse changed-region capture transaction");
    assert!(diff_action < changed_capture);
    assert!(!execute[diff_action..changed_capture].contains("capture_managed_surface("));
}

#[test]
fn native_visual_diff_result_exposes_only_evidence_ids_and_bounded_metadata() {
    let packet = include_str!("../src/visual_packet_impl.rs");
    let execute_start = packet
        .find("pub(crate) async fn execute_native_visual_packet(")
        .expect("native visual executor must exist");
    let execute = &packet[execute_start..];
    let diff_action = execute
        .find("NativeExecutorAction::VisualDiffCapture")
        .expect("visual diff action must exist");
    let diff_path = &execute[diff_action..];

    assert!(diff_path.contains("\"visual_diff_evidence_id\""));
    assert!(diff_path.contains("\"evidence_ids\""));
    assert!(diff_path.contains("\"mode\""));
    assert!(diff_path.contains("\"changed_ratio\""));
    assert!(diff_path.contains("\"baseline_cached\""));
    assert!(!diff_path.contains("artifact_id"));
    assert!(!diff_path.contains("png"));
    assert!(!diff_path.contains("verdict"));
}
