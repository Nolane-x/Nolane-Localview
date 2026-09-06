use std::collections::BTreeMap;

use localview_live_bridge::ConsequentialPostconditionStatus;
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
    SnapshotResourceUsage,
};
use localview_postcondition_contracts::{
    NativeSemanticCountComparisonV2, NativeSemanticNodeMatcherV2,
    NativeSemanticPostconditionContractV2,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsUiaPostconditionVerifier, WindowsUiaSemanticPostconditionVerifier,
};
use uuid::Uuid;

fn snapshot(
    completeness: ReconciliationCompleteness,
) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let provider = ProviderIncarnationRef::from("provider:uia-v2-verifier:1");
    let target = TargetIncarnationRef::from("target:uia-v2-verifier:1");
    let cut = "cut:uia-v2-verifier:after".to_owned();
    let nodes = (0..2)
        .map(|index| NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: provider.clone(),
                target_incarnation_ref: target.clone(),
                opaque_provider_element_id: format!("uia-runtime:status:{index}"),
                semantic_locator_hints: vec![],
                parent_surface_ref: Some("window:uia-v2-verifier".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("status".into()),
            name: Some(format!("Completed {}", index + 1)),
            control_type: Some("uia_control_type:50017".into()),
            automation_id: Some(format!("completion-status-{index}")),
            class_name: Some("Status".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::from([("state".into(), "ready".into())]),
        })
        .collect::<Vec<_>>();
    let mut cache = SemanticSnapshotCache::for_lineage(provider.clone(), target.clone());
    cache
        .publish(NativeSemanticSnapshotDraft {
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            snapshot_cut_ref: cut,
            surface_scope: "window:uia-v2-verifier".into(),
            cache_profile_revision: "windows-uia-control-view-v1".into(),
            permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
            capture_sequence: 1,
            nodes,
            resource_usage: SnapshotResourceUsage {
                nodes_observed: 2,
                properties_read: 16,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: completeness != ReconciliationCompleteness::Established,
            },
            completeness,
            incompleteness_debt: if completeness == ReconciliationCompleteness::Established {
                vec![]
            } else {
                vec!["enumeration:incomplete".into()]
            },
        })
        .unwrap()
}

fn at_least_two_ready_statuses() -> String {
    NativeSemanticPostconditionContractV2 {
        comparison: NativeSemanticCountComparisonV2::AtLeast,
        count: 2,
        matcher: NativeSemanticNodeMatcherV2 {
            role: Some("status".into()),
            attributes: BTreeMap::from([("state".into(), "ready".into())]),
            ..Default::default()
        },
    }
    .to_contract_ref()
    .unwrap()
}

#[test]
fn windows_verifier_consumes_shared_v2_cardinality_without_windows_owned_schema_logic() {
    let verifier = WindowsUiaSemanticPostconditionVerifier;
    let contract_ref = at_least_two_ready_statuses();
    let complete = snapshot(ReconciliationCompleteness::Established);

    let evidence = verifier
        .verify(
            Uuid::from_u128(0x8801),
            std::slice::from_ref(&contract_ref),
            complete.as_ref(),
        )
        .unwrap();

    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].contract_ref, contract_ref);
    assert_eq!(
        evidence[0].status,
        ConsequentialPostconditionStatus::VerifiedPass
    );
    assert!(!evidence[0].receipt_ref.trim().is_empty());
}

#[test]
fn windows_verifier_keeps_v2_cardinality_unknown_when_observation_is_incomplete() {
    let verifier = WindowsUiaSemanticPostconditionVerifier;
    let contract_ref = at_least_two_ready_statuses();
    let incomplete = snapshot(ReconciliationCompleteness::Incomplete);

    let evidence = verifier
        .verify(
            Uuid::from_u128(0x8802),
            &[contract_ref],
            incomplete.as_ref(),
        )
        .unwrap();

    assert_eq!(evidence.len(), 1);
    assert_eq!(
        evidence[0].status,
        ConsequentialPostconditionStatus::Unknown,
        "Windows adapter must preserve shared fail-closed V2 semantics"
    );
}
