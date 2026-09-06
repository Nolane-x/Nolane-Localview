use std::collections::BTreeMap;

use localview_live_bridge::ConsequentialPostconditionStatus;
use localview_native_provider::{
    NativeSemanticNodeMatcherV1, NativeSemanticNodeObservation,
    NativeSemanticPostconditionContractV1, NativeSemanticPostconditionExpectation,
    NativeSemanticSnapshotDraft, SemanticSnapshotCache, SnapshotResourceUsage,
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
    debt: Vec<String>,
) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let provider = ProviderIncarnationRef::from("provider:uia-verifier:1");
    let target = TargetIncarnationRef::from("target:uia-verifier:1");
    let cut = "cut:uia-verifier:after".to_owned();
    let node = NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            opaque_provider_element_id: "uia-runtime:[4,5,6]".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("window:uia-verifier".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("window".into()),
        name: Some("LocalView Runtime Invoked".into()),
        control_type: Some("uia_control_type:50032".into()),
        automation_id: None,
        class_name: Some("Window".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes: BTreeMap::new(),
    };
    let mut cache = SemanticSnapshotCache::for_lineage(provider.clone(), target.clone());
    cache
        .publish(NativeSemanticSnapshotDraft {
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            snapshot_cut_ref: cut,
            surface_scope: "window:uia-verifier".into(),
            cache_profile_revision: "windows-uia-control-view-v1".into(),
            permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
            capture_sequence: 1,
            nodes: vec![node],
            resource_usage: SnapshotResourceUsage {
                nodes_observed: 1,
                properties_read: 5,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: false,
            },
            completeness,
            incompleteness_debt: debt,
        })
        .unwrap()
}

fn title_contract(title: &str) -> String {
    NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: NativeSemanticNodeMatcherV1 {
            role: Some("window".into()),
            name: Some(title.into()),
            ..Default::default()
        },
    }
    .to_contract_ref()
    .unwrap()
}

#[test]
fn typed_verifier_proves_supported_contracts_and_keeps_unknown_refs_fail_closed() {
    let snapshot = snapshot(ReconciliationCompleteness::Established, vec![]);
    let expected = title_contract("LocalView Runtime Invoked");
    let missing = title_contract("Never Appeared");
    let legacy = "postcondition:legacy-opaque".to_owned();
    let verifier = WindowsUiaSemanticPostconditionVerifier;
    let action_id = Uuid::from_u128(0x8501);

    let evidence = verifier
        .verify(
            action_id,
            &[expected.clone(), missing.clone(), legacy.clone()],
            snapshot.as_ref(),
        )
        .unwrap();

    assert_eq!(evidence.len(), 3);
    assert_eq!(evidence[0].contract_ref, expected);
    assert_eq!(evidence[0].status, ConsequentialPostconditionStatus::VerifiedPass);
    assert!(!evidence[0].receipt_ref.trim().is_empty());
    assert_eq!(evidence[1].contract_ref, missing);
    assert_eq!(evidence[1].status, ConsequentialPostconditionStatus::VerifiedFail);
    assert_eq!(evidence[2].contract_ref, legacy);
    assert_eq!(
        evidence[2].status,
        ConsequentialPostconditionStatus::Unknown,
        "unsupported/legacy refs must remain unresolved rather than being guessed"
    );
}

#[test]
fn typed_verifier_never_proves_negative_or_positive_contracts_from_incomplete_snapshot() {
    let snapshot = snapshot(
        ReconciliationCompleteness::Incomplete,
        vec!["enumeration:incomplete".into()],
    );
    let present = title_contract("LocalView Runtime Invoked");
    let absent = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Absent,
        matcher: NativeSemanticNodeMatcherV1 {
            role: Some("dialog".into()),
            name: Some("Error".into()),
            ..Default::default()
        },
    }
    .to_contract_ref()
    .unwrap();
    let verifier = WindowsUiaSemanticPostconditionVerifier;

    let evidence = verifier
        .verify(
            Uuid::from_u128(0x8502),
            &[present, absent],
            snapshot.as_ref(),
        )
        .unwrap();

    assert!(evidence
        .iter()
        .all(|item| item.status == ConsequentialPostconditionStatus::Unknown));
}
