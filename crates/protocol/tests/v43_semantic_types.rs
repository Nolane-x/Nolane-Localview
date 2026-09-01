use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderElementRealization,
    ProviderElementRef, ProviderIncarnationRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, TargetIncarnationRef, TransportResult, WorldOutcome,
};
use serde_json::json;

#[test]
fn canonical_identity_refs_are_distinct_transparent_wire_types() {
    let principal = PrincipalRef::from("principal:agent-a");
    let provider = ProviderIncarnationRef::from("provider:webview:7");
    let target = TargetIncarnationRef::from("target:session:42");

    assert_eq!(serde_json::to_value(&principal).unwrap(), json!("principal:agent-a"));
    assert_eq!(serde_json::to_value(&provider).unwrap(), json!("provider:webview:7"));
    assert_eq!(serde_json::to_value(&target).unwrap(), json!("target:session:42"));
    assert_ne!(principal.as_str(), provider.as_str());
    assert_ne!(provider.as_str(), target.as_str());
}

#[test]
fn action_lifecycle_results_remain_orthogonal_on_the_wire() {
    assert_eq!(
        serde_json::to_value(TransportResult::DeliveredToExecutor).unwrap(),
        json!("delivered_to_executor")
    );
    assert_eq!(
        serde_json::to_value(DispatchResult::DispatchedPartial).unwrap(),
        json!("dispatched_partial")
    );
    assert_eq!(
        serde_json::to_value(WorldOutcome::ReconciliationRequired).unwrap(),
        json!("reconciliation_required")
    );

    // Transport delivery and even a full provider dispatch cannot imply a verified world outcome.
    assert_ne!(
        serde_json::to_value(DispatchResult::DispatchedFull).unwrap(),
        serde_json::to_value(WorldOutcome::VerifiedExpected).unwrap()
    );
}

#[test]
fn reconnect_is_not_continuity_and_reconciliation_is_a_separate_axis() {
    let receipt = ReconciliationSnapshotReceipt {
        receipt_id: "reconcile:9".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("provider:2"),
        target_incarnation_ref: TargetIncarnationRef::from("target:4"),
        snapshot_cut_ref: "cut:18".into(),
        surface_scope: "active_dialog".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:v3".into(),
        permission_visibility_revision: "permission:v8".into(),
        capture_sequence: 81,
        observed_digest: "sha256:abc".into(),
        incompleteness_debt: Vec::new(),
    };

    assert_eq!(
        serde_json::to_value(EventContinuityState::ReconnectedUnreconciled).unwrap(),
        json!("reconnected_unreconciled")
    );
    assert_eq!(receipt.completeness, ReconciliationCompleteness::Established);
}

#[test]
fn provider_element_identity_is_bound_to_provider_and_target_incarnations() {
    let old = ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: ProviderIncarnationRef::from("uia:worker:1"),
        target_incarnation_ref: TargetIncarnationRef::from("notepad:1"),
        opaque_provider_element_id: "runtime-id:7".into(),
        semantic_locator_hints: vec!["role=textbox".into(), "name=Document".into()],
        parent_surface_ref: Some("window:main".into()),
        acquisition_cut_ref: "cut:10".into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "uia-lifetime:v1".into(),
    };
    let reincarnated = ProviderElementRef {
        provider_incarnation_ref: ProviderIncarnationRef::from("uia:worker:2"),
        ..old.clone()
    };

    assert_ne!(old, reincarnated);
    assert_eq!(old.opaque_provider_element_id, reincarnated.opaque_provider_element_id);
}
