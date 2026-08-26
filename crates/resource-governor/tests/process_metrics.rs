use localview_resource_governor::{
    DegradationAction, RuntimeResourceGovernor, RuntimeResourceSample,
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
