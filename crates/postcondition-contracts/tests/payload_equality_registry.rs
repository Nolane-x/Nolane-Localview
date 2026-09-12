use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
    SnapshotResourceUsage,
};
use localview_postcondition_contracts::{
    NativeSemanticPostconditionEvaluation, PayloadEqualityModeV1,
    PayloadEqualityPostconditionContractError, PayloadEqualityPostconditionContractV1,
    PostconditionContractRegistry, PostconditionContractRegistryError,
    RegisteredPostconditionContract,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, TargetIncarnationRef,
};

const PAYLOAD_REF: &str = "8bdc6fd9-77dc-4e4c-a3ae-df895d02584d";

fn complete_snapshot() -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision>
{
    let provider = ProviderIncarnationRef::from("provider:payload-equality:test");
    let target = TargetIncarnationRef::from("target:payload-equality:test");
    let cut = "cut:payload-equality:1".to_owned();
    let mut cache = SemanticSnapshotCache::for_lineage(provider.clone(), target.clone());
    cache
        .publish(NativeSemanticSnapshotDraft {
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            snapshot_cut_ref: cut.clone(),
            surface_scope: "surface:payload-equality".into(),
            cache_profile_revision: "payload-equality-test-v1".into(),
            permission_visibility_revision: "payload-equality-visible-v1".into(),
            capture_sequence: 1,
            nodes: vec![NativeSemanticNodeObservation {
                element_ref: ProviderElementRef {
                    provider_family: "native_test".into(),
                    provider_incarnation_ref: provider,
                    target_incarnation_ref: target,
                    opaque_provider_element_id: "element:payload-equality".into(),
                    semantic_locator_hints: vec![],
                    parent_surface_ref: Some("surface:payload-equality".into()),
                    acquisition_cut_ref: cut,
                    realization: ProviderElementRealization::RealizedCurrent,
                    lifetime_profile_revision: "native-test-v1".into(),
                },
                parent_index: None,
                depth: 0,
                role: Some("textbox".into()),
                name: Some("Editable".into()),
                control_type: Some("edit".into()),
                automation_id: Some("payload-equality-edit".into()),
                class_name: Some("Edit".into()),
                is_enabled: Some(true),
                is_offscreen: Some(false),
                attributes: BTreeMap::new(),
            }],
            resource_usage: SnapshotResourceUsage {
                nodes_observed: 1,
                properties_read: 8,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: false,
            },
            completeness: ReconciliationCompleteness::Established,
            incompleteness_debt: vec![],
        })
        .unwrap()
}

fn replace_contract() -> PayloadEqualityPostconditionContractV1 {
    PayloadEqualityPostconditionContractV1 {
        mode: PayloadEqualityModeV1::ReplaceValue,
        payload_ref: PAYLOAD_REF.into(),
    }
}

#[test]
fn payload_equality_v1_round_trips_canonically_through_standard_registry() {
    let contract = replace_contract();
    let encoded = contract.to_contract_ref().unwrap();
    assert_eq!(
        encoded,
        format!(
            "lvpc:payload-equality:v1:{{\"mode\":\"replace_value\",\"payload_ref\":\"{PAYLOAD_REF}\"}}"
        )
    );

    let registry = PostconditionContractRegistry::standard();
    assert!(
        registry
            .schemas()
            .iter()
            .any(|schema| schema.family == "payload-equality" && schema.version == "1")
    );
    assert_eq!(
        registry.decode(&encoded).unwrap(),
        RegisteredPostconditionContract::PayloadEqualityV1(contract.clone())
    );
    assert_eq!(
        PayloadEqualityPostconditionContractV1::from_contract_ref(&encoded).unwrap(),
        contract
    );
}

#[test]
fn payload_equality_v1_rejects_unknown_fields_bad_uuid_and_noncanonical_wire() {
    let valid = replace_contract().to_contract_ref().unwrap();
    let unknown = valid.replacen(
        "\"payload_ref\":",
        "\"business_success\":true,\"payload_ref\":",
        1,
    );
    assert!(matches!(
        PayloadEqualityPostconditionContractV1::from_contract_ref(&unknown),
        Err(PayloadEqualityPostconditionContractError::UnknownField)
    ));

    let bad_uuid = valid.replace(PAYLOAD_REF, "not-a-uuid");
    assert!(matches!(
        PayloadEqualityPostconditionContractV1::from_contract_ref(&bad_uuid),
        Err(PayloadEqualityPostconditionContractError::InvalidPayloadRef)
    ));

    let noncanonical = format!(
        "lvpc:payload-equality:v1:{{\"payload_ref\":\"{PAYLOAD_REF}\",\"mode\":\"replace_value\"}}"
    );
    assert!(matches!(
        PayloadEqualityPostconditionContractV1::from_contract_ref(&noncanonical),
        Err(PayloadEqualityPostconditionContractError::NonCanonicalReference)
    ));
}

#[test]
fn generic_semantic_snapshot_can_never_prove_payload_equality() {
    let registry = PostconditionContractRegistry::standard();
    let encoded = replace_contract().to_contract_ref().unwrap();
    let snapshot = complete_snapshot();

    assert_eq!(
        registry
            .evaluate_native_semantic(&encoded, snapshot.as_ref())
            .unwrap(),
        NativeSemanticPostconditionEvaluation::Unknown
    );

    assert!(matches!(
        registry.decode("lvpc:payload-equality:v2:{}"),
        Err(PostconditionContractRegistryError::UnsupportedVersion { family, version })
            if family == "payload-equality" && version == "2"
    ));
}
