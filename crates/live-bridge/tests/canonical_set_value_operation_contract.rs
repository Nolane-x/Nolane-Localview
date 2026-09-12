use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, LiveBridge, ProviderObserverBatch,
};
use localview_protocol::{PrincipalRef, ProviderIncarnationRef, TargetIncarnationRef};
use uuid::Uuid;

#[test]
fn set_value_is_a_distinct_payload_free_canonical_operation() {
    assert_eq!(
        format!("{:?}", CanonicalActionOperation::SetValue),
        "SetValue"
    );
    assert_eq!(
        CanonicalActionOperation::from_bridge_action_kind(&BridgeActionKind::TypeText {
            text: "caller-secret".into(),
            clear_first: false,
        }),
        Some(CanonicalActionOperation::InputText),
        "legacy TypeText must remain InputText and must never synthesize SetValue authority",
    );
}

#[tokio::test]
async fn set_value_compatibility_carrier_can_remain_empty_and_private() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::from_u128(0x5e7_0001);
    let provider = ProviderIncarnationRef::from("provider:windows-uia:set-value-operation");
    let target = TargetIncarnationRef::from("target:windows:set-value-operation");
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: Vec::new(),
        })
        .await;

    let caller_plaintext = "caller-secret-must-not-enter-carrier";
    let direct = bridge
        .bind_direct_canonical_action(
            session_id,
            Some("windows-uia:exact-edit".into()),
            BridgeActionKind::TypeText {
                text: String::new(),
                clear_first: false,
            },
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from("principal:local-control:user"),
                acting_principal_ref: PrincipalRef::from("principal:localview-daemon:windows-uia"),
                authorization_revision: "authorization:set-value-operation:v1".into(),
                precondition_snapshot_cut_ref: "cut:set-value-operation:1".into(),
                provider_incarnation_ref: provider,
                target_incarnation_ref: target,
                risk_class: ActionRiskClass::DestructiveOrIrreversible,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![
                    "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"class_name\":\"Edit\"}}".into(),
                ],
            },
        )
        .await
        .expect("direct payload-free compatibility carrier should bind");

    match &direct.action.action {
        BridgeActionKind::TypeText { text, .. } => assert!(text.is_empty()),
        other => panic!("unexpected compatibility carrier: {other:?}"),
    }
    assert!(
        !format!("{:?}", direct.action).contains(caller_plaintext),
        "caller plaintext must not enter the compatibility carrier",
    );
    assert!(
        bridge.take_public_actions(session_id, 8).await.is_empty(),
        "direct SetValue compatibility carrier must never enter the legacy public queue",
    );
}
