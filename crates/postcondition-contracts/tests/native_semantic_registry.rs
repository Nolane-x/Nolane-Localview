use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
    SnapshotResourceUsage,
};
use localview_postcondition_contracts::{
    NativeSemanticCountComparisonV2, NativeSemanticNodeMatcherV1, NativeSemanticNodeMatcherV2,
    NativeSemanticPostconditionContractError, NativeSemanticPostconditionContractV1,
    NativeSemanticPostconditionContractV2, NativeSemanticPostconditionEvaluation,
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
    snapshot_with_matching_nodes(completeness, debt, 1)
}

fn snapshot_with_matching_nodes(
    completeness: ReconciliationCompleteness,
    debt: Vec<String>,
    matching_nodes: usize,
) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let provider = ProviderIncarnationRef::from("provider:shared-postcondition:1");
    let target = TargetIncarnationRef::from("target:shared-postcondition:1");
    let cut = "cut:shared-postcondition:after".to_owned();
    let nodes = (0..matching_nodes)
        .map(|index| NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "native_test".into(),
                provider_incarnation_ref: provider.clone(),
                target_incarnation_ref: target.clone(),
                opaque_provider_element_id: format!("element:{}", index + 1),
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
            automation_id: Some(format!("completion-status-{}", index + 1)),
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
            surface_scope: "surface:shared-postcondition".into(),
            cache_profile_revision: "native-test-cache-v1".into(),
            permission_visibility_revision: "native-test-visible-v1".into(),
            capture_sequence: 1,
            nodes,
            resource_usage: SnapshotResourceUsage {
                nodes_observed: matching_nodes,
                properties_read: matching_nodes.saturating_mul(8),
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

fn completion_count_contract(
    comparison: NativeSemanticCountComparisonV2,
    count: u32,
) -> NativeSemanticPostconditionContractV2 {
    NativeSemanticPostconditionContractV2 {
        comparison,
        count,
        matcher: NativeSemanticNodeMatcherV2 {
            role: Some("status".into()),
            attributes: BTreeMap::from([("state".into(), "ready".into())]),
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
    assert_eq!(registry.schemas().len(), 3);
    assert_eq!(registry.schemas()[0].family, "native-semantic");
    assert_eq!(registry.schemas()[0].version, "1");
    assert_eq!(registry.schemas()[1].family, "native-semantic");
    assert_eq!(registry.schemas()[1].version, "2");
    assert_eq!(registry.schemas()[2].family, "payload-equality");
    assert_eq!(registry.schemas()[2].version, "1");

    assert!(matches!(
        registry.decode("lvpc:browser-dom:v1:{}"),
        Err(PostconditionContractRegistryError::UnsupportedFamily { family })
            if family == "browser-dom"
    ));
    assert!(matches!(
        registry.decode("lvpc:native-semantic:v3:{}"),
        Err(PostconditionContractRegistryError::UnsupportedVersion { family, version })
            if family == "native-semantic" && version == "3"
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
fn native_semantic_v2_cardinality_is_canonical_and_registered_without_changing_v1() {
    let contract = completion_count_contract(NativeSemanticCountComparisonV2::AtLeast, 2);
    let encoded = contract.to_contract_ref().unwrap();
    assert_eq!(
        encoded,
        r#"lvpc:native-semantic:v2:{"comparison":"at_least","count":2,"matcher":{"role":"status","attributes":{"state":"ready"}}}"#
    );

    let registry = PostconditionContractRegistry::standard();
    assert_eq!(
        registry.decode(&encoded).unwrap(),
        RegisteredPostconditionContract::NativeSemanticV2(contract.clone())
    );
    assert_eq!(
        NativeSemanticPostconditionContractV2::from_contract_ref(&encoded).unwrap(),
        contract
    );
    assert_eq!(
        completion_contract().to_contract_ref().unwrap(),
        r#"lvpc:native-semantic:v1:{"expectation":"present","matcher":{"name":"Completed"}}"#
    );
}

#[test]
fn native_semantic_v2_compares_exact_multi_node_cardinality_on_complete_snapshot() {
    let registry = PostconditionContractRegistry::standard();
    let complete = snapshot_with_matching_nodes(ReconciliationCompleteness::Established, vec![], 2);

    for (comparison, count, expected) in [
        (
            NativeSemanticCountComparisonV2::Equal,
            2,
            NativeSemanticPostconditionEvaluation::VerifiedPass,
        ),
        (
            NativeSemanticCountComparisonV2::AtLeast,
            2,
            NativeSemanticPostconditionEvaluation::VerifiedPass,
        ),
        (
            NativeSemanticCountComparisonV2::AtMost,
            2,
            NativeSemanticPostconditionEvaluation::VerifiedPass,
        ),
        (
            NativeSemanticCountComparisonV2::Equal,
            1,
            NativeSemanticPostconditionEvaluation::VerifiedFail,
        ),
        (
            NativeSemanticCountComparisonV2::AtLeast,
            3,
            NativeSemanticPostconditionEvaluation::VerifiedFail,
        ),
        (
            NativeSemanticCountComparisonV2::AtMost,
            1,
            NativeSemanticPostconditionEvaluation::VerifiedFail,
        ),
    ] {
        let encoded = completion_count_contract(comparison, count)
            .to_contract_ref()
            .unwrap();
        assert_eq!(
            registry
                .evaluate_native_semantic(&encoded, complete.as_ref())
                .unwrap(),
            expected
        );
    }
}

#[test]
fn native_semantic_v2_can_prove_zero_only_from_complete_observation() {
    let registry = PostconditionContractRegistry::standard();
    let complete = snapshot_with_matching_nodes(ReconciliationCompleteness::Established, vec![], 0);
    let absent = completion_count_contract(NativeSemanticCountComparisonV2::Equal, 0)
        .to_contract_ref()
        .unwrap();
    assert_eq!(
        registry
            .evaluate_native_semantic(&absent, complete.as_ref())
            .unwrap(),
        NativeSemanticPostconditionEvaluation::VerifiedPass
    );

    let incomplete = snapshot_with_matching_nodes(
        ReconciliationCompleteness::Incomplete,
        vec!["enumeration:incomplete".into()],
        0,
    );
    assert_eq!(
        registry
            .evaluate_native_semantic(&absent, incomplete.as_ref())
            .unwrap(),
        NativeSemanticPostconditionEvaluation::Unknown
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

#[test]
fn direct_v2_parser_rejects_unknown_fields_and_noncanonical_references() {
    let encoded = completion_count_contract(NativeSemanticCountComparisonV2::Equal, 1)
        .to_contract_ref()
        .unwrap();
    let unknown_field = encoded.replacen(
        "\"count\":1,",
        "\"count\":1,\"business_success\":true,",
        1,
    );
    assert!(matches!(
        NativeSemanticPostconditionContractV2::from_contract_ref(&unknown_field),
        Err(NativeSemanticPostconditionContractError::UnknownField)
    ));

    let non_canonical = encoded.replacen(
        "lvpc:native-semantic:v2:{",
        "lvpc:native-semantic:v2: {",
        1,
    );
    assert!(matches!(
        NativeSemanticPostconditionContractV2::from_contract_ref(&non_canonical),
        Err(NativeSemanticPostconditionContractError::NonCanonicalReference)
    ));
}
