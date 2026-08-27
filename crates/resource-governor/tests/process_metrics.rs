use localview_resource_governor::{
    normalize_process_metrics, DegradationAction, RuntimeResourceGovernor, RuntimeResourceSample,
};

#[test]
fn daemon_process_metrics_aggregate_with_reported_runtime_metrics() {
    let governor = RuntimeResourceGovernor::default();
    assert!(governor.update_sample(RuntimeResourceSample {
        memory_mb: 180,
        cpu_percent: 2.0,
        capture_storage_mb: 32,
        network_kb_per_minute: 12,
    }));
    assert!(governor.update_process_metrics(100, 2.0));

    let decision = governor.decision();
    assert!(
        decision
            .actions
            .contains(&DegradationAction::PreferSemanticOverVisual),
        "daemon memory plus externally reported runtime memory must share one budget"
    );
}

#[test]
fn invalid_process_cpu_samples_are_rejected_without_poisoning_authority() {
    let governor = RuntimeResourceGovernor::default();
    assert!(!governor.update_process_metrics(10, f32::NAN));
    assert!(governor.update_process_metrics(10, 1.0));
}

#[test]
fn raw_process_metrics_are_normalized_to_machine_share_and_never_undercount_memory() {
    let normalized = normalize_process_metrics(64 * 1024 * 1024 + 1, 40.0, 8)
        .expect("valid process metrics");

    assert_eq!(normalized.memory_mb, 65);
    assert!((normalized.cpu_percent - 5.0).abs() <= f32::EPSILON);
}

#[test]
fn raw_process_metric_normalization_rejects_non_finite_cpu_and_handles_unknown_parallelism() {
    assert!(normalize_process_metrics(1024, f32::NAN, 8).is_none());
    let normalized = normalize_process_metrics(1024, 3.0, 0).expect("zero CPU count falls back to one");
    assert_eq!(normalized.memory_mb, 1);
    assert!((normalized.cpu_percent - 3.0).abs() <= f32::EPSILON);
}
