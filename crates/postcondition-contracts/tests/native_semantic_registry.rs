use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
    SnapshotResourceUsage,
};
use localview_postcondition_contracts::{
    NativeSemanticNodeMatcherV1, NativeSemanticPostconditionContractError,
    NativeSemanticPostconditionContractV1, NativeSemanticPostconditionEvaluation,
    NativeSemanticPostconditionExpectation, PostconditionContractRegistry,
    PostconditionContractRegistryError, RegisteredPostconditionContract,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, TargetIncarnationRef,
};

fn snapshot(
    completeness: ReconciliationCompleteness,
    debt: Vec<String>,
) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let provider = ProviderIncarnationRef::from("provider:shared-postcondition:1");
    let target = TargetIncarnationRef::from("target:shared-postcondition:1");
    let cut = "cut:shared-postcondition:after".to_owned();
    let node = NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "native_test".into(),
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            opaque_provider_element_id: "element:1".into(),
            semantic_locator_hints: vec![],
            parent_surface_ref: Some("surface:shared-postcondition".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "native-test-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("status".into()),
        name: Some("Completed".into()),
        control_type: Some("status".into()),
        automation_id: Some("completion-status".into()),
        class_name: Some("Status".into()),
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
            surface_scope: "surface:shared-postcondition".into(),
            cache_profile_revision: "native-test-cache-v1".into(),
            permission_visibility_revision: "native-test-visible-v1".into(),
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

fn completion_contract() -> NativeSemanticPostconditionContractV1 {
    NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: NativeSemanticNodeMatcherV1 {
            name: Some("Completed".into()),
            ..Default::default()
        },
    }
}

#[test]
fn standard_registry_preserves_existing_native_semantic_v1_wire_bytes() {
    let encoded = completion_contract().to_contract_ref().unwrap();
    assert_eq!(
        encoded,
        r#"lvpc:native-semantic:v1:{"expectation":"present","matcher":{"name":"Completed"}}"#
    );

    let registry = PostconditionContractRegistry::standard();
    let decoded = registry.decode(&encoded).unwrap();
    assert_eq!(
        decoded,
        RegisteredPostconditionContract::NativeSemanticV1(completion_contract())
    );
}

#[test]
fn standard_registry_is_explicit_and_rejects_unregistered_family_or_version() {
    let registry = PostconditionContractRegistry::standard();
    assert_eq!(registry.schemas().len(), 1);
    assert_eq!(registry.schemas()[0].family, "native-semantic");
    assert_eq!(registry.schemas()[0].version, "1");

    assert!(matches!(
        registry.decode("lvpc:browser-dom:v1:{}"),
        Err(PostconditionContractRegistryError::UnsupportedFamily { family })
            if family == "browser-dom"
    ));
    assert!(matches!(
        registry.decode("lvpc:native-semantic:v2:{}"),
        Err(PostconditionContractRegistryError::UnsupportedVersion { family, version })
            if family == "native-semantic" && version == "2"
    ));
}

#[test]
fn registry_evaluates_shared_native_semantic_contract_without_windows_types() {
    let registry = PostconditionContractRegistry::standard();
    let complete = snapshot(ReconciliationCompleteness::Established, vec![]);
    let encoded = completion_contract().to_contract_ref().unwrap();

    assert_eq!(
        registry
            .evaluate_native_semantic(&encoded, complete.as_ref())
            .unwrap(),
        NativeSemanticPostconditionEvaluation::VerifiedPass
    );

    let missing = NativeSemanticPostconditionContractV1 {
        expectation: NativeSemanticPostconditionExpectation::Present,
        matcher: NativeSemanticNodeMatcherV1 {
            name: Some("Never Appeared".into()),
            ..Default::default()
        },
    }
    .to_contract_ref()
    .unwrap();
    assert_eq!(
        registry
            .evaluate_native_semantic(&missing, complete.as_ref())
            .unwrap(),
        NativeSemanticPostconditionEvaluation::VerifiedFail
    );
}

#[test]
fn shared_registry_never_proves_presence_or_absence_from_incomplete_observation() {
    let registry = PostconditionContractRegistry::standard();
    let incomplete = snapshot(
        ReconciliationCompleteness::Incomplete,
        vec!["enumeration:incomplete".into()],
    );
    let present = completion_contract().to_contract_ref().unwrap();
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

    for contract_ref in [present, absent] {
        assert_eq!(
            registry
                .evaluate_native_semantic(&contract_ref, incomplete.as_ref())
                .unwrap(),
            NativeSemanticPostconditionEvaluation::Unknown
        );
    }
}

#[test]
fn direct_v1_parser_keeps_strict_legacy_error_semantics() {
    let encoded = completion_contract().to_contract_ref().unwrap();
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
}
