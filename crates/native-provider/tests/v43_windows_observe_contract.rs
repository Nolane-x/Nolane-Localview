use localview_native_provider::{
    derive_windows_target_incarnation, provider_element_ref_from_runtime_id,
    NativeProviderCapabilities, ProviderEventOrdering, ProviderEventReliabilityProfile,
    SnapshotBudget, SnapshotBudgetGuard, SnapshotBudgetLimit, UserSelectedWindowTarget,
    WindowsTargetFingerprint,
};
use localview_protocol::{ProviderIncarnationRef, ProviderElementRealization};
use uuid::Uuid;

#[test]
fn explicit_user_selection_and_process_lifetime_bind_target_incarnation() {
    let selection = UserSelectedWindowTarget {
        native_window_handle: 0x1234,
        expected_process_id: 77,
        selection_nonce: Uuid::new_v4(),
    };
    let first = WindowsTargetFingerprint {
        native_window_handle: 0x1234,
        process_id: 77,
        process_start_time_ticks: 100,
        root_runtime_id_hint: vec![42, 7],
    };
    let reincarnated = WindowsTargetFingerprint {
        process_start_time_ticks: 200,
        ..first.clone()
    };

    let first_ref = derive_windows_target_incarnation(&selection, &first).unwrap();
    let reincarnated_ref = derive_windows_target_incarnation(&selection, &reincarnated).unwrap();
    assert_ne!(first_ref, reincarnated_ref);
}

#[test]
fn runtime_id_is_only_an_opaque_hint_inside_provider_and_target_incarnation() {
    let provider = ProviderIncarnationRef::from("provider:windows-uia:worker-1");
    let first_target = localview_protocol::TargetIncarnationRef::from("target:first");
    let second_target = localview_protocol::TargetIncarnationRef::from("target:second");

    let first = provider_element_ref_from_runtime_id(
        provider.clone(),
        first_target,
        &[42, 7],
        "cut:1",
        ProviderElementRealization::RealizedCurrent,
    );
    let reused = provider_element_ref_from_runtime_id(
        provider,
        second_target,
        &[42, 7],
        "cut:2",
        ProviderElementRealization::RealizedCurrent,
    );

    assert_eq!(first.opaque_provider_element_id, reused.opaque_provider_element_id);
    assert_ne!(first.target_incarnation_ref, reused.target_incarnation_ref);
    assert_ne!(first, reused);
}

#[test]
fn windows_uia_reliability_never_claims_property_events_are_complete() {
    let profile = ProviderEventReliabilityProfile::windows_uia_v1();
    assert_eq!(profile.ordering, ProviderEventOrdering::OpaqueBestEffort);
    assert!(!profile.property_change_events_complete);
    assert!(profile.action_critical_properties_require_reconciliation);
    assert!(!profile.global_polling_required);
}

#[test]
fn snapshot_accounting_fails_bounded_and_reports_the_exhausted_dimension() {
    let mut guard = SnapshotBudgetGuard::new(SnapshotBudget {
        max_nodes: 3,
        max_depth: 2,
        max_properties: 10,
    });

    assert!(guard.admit_node(0, 3));
    assert!(guard.admit_node(1, 3));
    assert!(guard.admit_node(2, 3));
    assert!(!guard.admit_node(2, 1));
    assert!(!guard.admit_node(3, 1));

    let usage = guard.finish();
    assert_eq!(usage.nodes_observed, 3);
    assert_eq!(usage.properties_read, 9);
    assert!(usage.exhausted.contains(&SnapshotBudgetLimit::Nodes));
    assert!(usage.exhausted.contains(&SnapshotBudgetLimit::Depth));
    assert!(usage.incomplete);
}

#[test]
fn phase_five_provider_capabilities_are_observe_only() {
    let capabilities = NativeProviderCapabilities::windows_observe_only();
    assert!(capabilities.semantic_snapshot);
    assert!(capabilities.event_subscription);
    assert!(capabilities.reconciliation);
    assert!(capabilities.resource_accounting);
    assert!(!capabilities.write_actions);
    assert!(!capabilities.input_dispatch);
}
