use localview_resource_governor::{
    DegradationAction, PressureLevel, ResourceWorkKind, RuntimeResourceGovernor,
    RuntimeResourceSample,
};

#[test]
fn native_semantic_work_is_admitted_at_normal_pressure() {
    let governor = RuntimeResourceGovernor::default();

    assert!(
        governor
            .check(ResourceWorkKind::NativeSemanticObservation)
            .is_ok()
    );
    assert!(
        governor
            .check(ResourceWorkKind::NativeSemanticReconciliation)
            .is_ok()
    );
}

#[test]
fn high_pressure_prefers_bounded_semantic_work_over_visual_and_chromium() {
    let governor = RuntimeResourceGovernor::default();
    assert!(governor.update_sample(RuntimeResourceSample {
        memory_mb: 300,
        cpu_percent: 4.0,
        capture_storage_mb: 12,
        network_kb_per_minute: 24,
    }));

    let observation = governor
        .check(ResourceWorkKind::NativeSemanticObservation)
        .expect("high pressure should still admit bounded semantic observation");
    let reconciliation = governor
        .check(ResourceWorkKind::NativeSemanticReconciliation)
        .expect("required reconciliation should remain admissible below critical pressure");

    assert_eq!(observation.pressure, PressureLevel::High);
    assert_eq!(reconciliation.pressure, PressureLevel::High);
    assert!(
        observation
            .actions
            .contains(&DegradationAction::PreferSemanticOverVisual)
    );
    assert!(
        governor
            .check(ResourceWorkKind::NativeVisualCapture)
            .is_err()
    );
    assert!(governor.check(ResourceWorkKind::Chromium).is_err());
}

#[test]
fn critical_pressure_defers_semantic_work_without_turning_it_into_success() {
    let governor = RuntimeResourceGovernor::default();
    assert!(governor.update_sample(RuntimeResourceSample {
        memory_mb: 600,
        cpu_percent: 30.0,
        capture_storage_mb: 12,
        network_kb_per_minute: 24,
    }));

    let observation = governor
        .check(ResourceWorkKind::NativeSemanticObservation)
        .unwrap_err();
    let reconciliation = governor
        .check(ResourceWorkKind::NativeSemanticReconciliation)
        .unwrap_err();

    assert_eq!(observation.decision.pressure, PressureLevel::Critical);
    assert_eq!(reconciliation.decision.pressure, PressureLevel::Critical);
    assert_eq!(
        observation.work_kind,
        ResourceWorkKind::NativeSemanticObservation
    );
    assert_eq!(
        reconciliation.work_kind,
        ResourceWorkKind::NativeSemanticReconciliation
    );
}

#[test]
fn semantic_resource_work_kinds_have_stable_wire_names() {
    assert_eq!(
        serde_json::to_value(ResourceWorkKind::NativeSemanticObservation).unwrap(),
        serde_json::json!("native_semantic_observation")
    );
    assert_eq!(
        serde_json::to_value(ResourceWorkKind::NativeSemanticReconciliation).unwrap(),
        serde_json::json!("native_semantic_reconciliation")
    );
}
