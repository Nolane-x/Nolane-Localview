use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeMatcherV1, NativeSemanticNodeObservation,
    NativeSemanticPostconditionContractError, NativeSemanticPostconditionContractV1,
    NativeSemanticPostconditionEvaluation, NativeSemanticPostconditionExpectation,
    NativeSemanticSnapshotDraft, SemanticSnapshotCache, SnapshotResourceUsage,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, TargetIncarnationRef,
};

fn snapshot(
    completeness: ReconciliationCompleteness,
    debt: Vec<String>,
) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let provider = ProviderIncarnationRef::from("provider:contract-test:1");
    let target = TargetIncarnationRef::from("target:contract-test:1");
    let cut = "cut:contract-test:after".to_owned();
    let node = NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            opaque_provider_element_id: "uia-runtime:[1,2,3]".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("window:contract-test".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("window".into()),
        name: Some("LocalView Runtime Invoked".into()),
        control_type: Some("uia_control_type:50032".into()),
        automation_id: Some("runtime-window".into()),
        class_name: Some("Window".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes: BTreeMap::from([("state".into(), "ready".into())]),
    };
    let mut cache = SemanticSnapshotCache::for_lineage(provider.clone(), target.clone());
    cache
        .publish(NativeSemanticSnapshotDraft {
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            snapshot_cut_ref: cut,
            surface_scope: "window:contract-test".into(),
            cache_profile_revision: "windows-uia-control-view-v1".into(),
            permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
            capture_sequence: 1,
            nodes: vec![node],
            resource_usage: SnapshotResourceUsage {
                nodes_observed: 1,
                properties_read: 8,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: false,
            },
            completeness,
            incompleteness_debt: debt,
        })
        .unwrap()
}

fn invoked_window_matcher() -> NativeSemanticNodeMatcherV1 {
    NativeSemanticNodeMatcherV1 {
        role: Some("window".into()),
        name: Some("LocalView Runtime Invoked".into()),
        control_type: Some("uia_control_type:50032".into()),
        automation_id: Some("runtime-window".into()),
        class_name: Some("Window".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes: BTreeMap::from([("state".into(), "ready".into())]),
    }
}

#[test]
fn contract_ref_is_versioned_canonical_and_rejects_ambiguous_forms() {
    let contract = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: invoked_window_matcher(),
    };

    let encoded = contract.to_contract_ref().unwrap();
    assert!(encoded.starts_with("lvpc:native-semantic:v1:"));
    assert_eq!(
        NativeSemanticPostconditionContractV1::from_contract_ref(&encoded).unwrap(),
        contract
    );

    let unsupported = encoded.replacen(
        "lvpc:native-semantic:v1:",
        "lvpc:native-semantic:v2:",
        1,
    );
    assert!(matches!(
        NativeSemanticPostconditionContractV1::from_contract_ref(&unsupported),
        Err(NativeSemanticPostconditionContractError::UnsupportedVersion { .. })
    ));

    let non_canonical = encoded.replacen(
        "lvpc:native-semantic:v1:{",
        "lvpc:native-semantic:v1: {",
        1,
    );
    assert!(matches!(
        NativeSemanticPostconditionContractV1::from_contract_ref(&non_canonical),
        Err(NativeSemanticPostconditionContractError::NonCanonicalReference)
    ));

    let empty = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: NativeSemanticNodeMatcherV1::default(),
    };
    assert!(matches!(
        empty.to_contract_ref(),
        Err(NativeSemanticPostconditionContractError::EmptyMatcher)
    ));
}

#[test]
fn complete_snapshot_can_prove_presence_absence_and_failure_but_incomplete_is_unknown() {
    let complete = snapshot(ReconciliationCompleteness::Established, vec![]);
    let present = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: invoked_window_matcher(),
    };
    assert_eq!(
        present.evaluate(complete.as_ref()),
        NativeSemanticPostconditionEvaluation::VerifiedPass
    );

    let absent_error = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Absent,
        matcher: NativeSemanticNodeMatcherV1 {
            role: Some("dialog".into()),
            name: Some("Error".into()),
            ..Default::default()
        },
    };
    assert_eq!(
        absent_error.evaluate(complete.as_ref()),
        NativeSemanticPostconditionEvaluation::VerifiedPass
    );

    let missing_expected = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: NativeSemanticNodeMatcherV1 {
            name: Some("Never Appeared".into()),
            ..Default::default()
        },
    };
    assert_eq!(
        missing_expected.evaluate(complete.as_ref()),
        NativeSemanticPostconditionEvaluation::VerifiedFail
    );

    let incomplete = snapshot(
        ReconciliationCompleteness::Incomplete,
        vec!["enumeration:incomplete".into()],
    );
    assert_eq!(
        present.evaluate(incomplete.as_ref()),
        NativeSemanticPostconditionEvaluation::Unknown
    );
    assert_eq!(
        absent_error.evaluate(incomplete.as_ref()),
        NativeSemanticPostconditionEvaluation::Unknown,
        "negative absence must never be inferred from incomplete enumeration"
    );
}
