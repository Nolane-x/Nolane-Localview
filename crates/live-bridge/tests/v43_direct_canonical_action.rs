use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind, LiveBridge,
    ProviderObserverBatch,
};
use localview_protocol::{PrincipalRef, ProviderIncarnationRef, TargetIncarnationRef};
use uuid::Uuid;

#[tokio::test]
async fn direct_canonical_action_is_bound_without_entering_legacy_public_queue() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::from_u128(0x9101);
    let provider = ProviderIncarnationRef::from("provider:windows-uia:direct-canonical");
    let target = TargetIncarnationRef::from("target:windows:direct-canonical");
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: Vec::new(),
        })
        .await;

    let direct = bridge
        .bind_direct_canonical_action(
            session_id,
            Some("windows-uia:exact-element".into()),
            BridgeActionKind::Click,
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from("principal:local-control:user"),
                acting_principal_ref: PrincipalRef::from("principal:localview-daemon:windows-uia"),
                authorization_revision: "authorization:direct-canonical:v1".into(),
                precondition_snapshot_cut_ref: "cut:direct-canonical:1".into(),
                provider_incarnation_ref: provider,
                target_incarnation_ref: target,
                risk_class: ActionRiskClass::DestructiveOrIrreversible,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![
                    "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"name\":\"Done\"}}".into(),
                ],
            },
        )
        .await
        .expect("direct canonical action should bind exact authority");

    assert_eq!(direct.action.id, direct.envelope.transport_action_id);
    assert_eq!(
        bridge.action_envelope(direct.action.id).await,
        Some(direct.envelope.clone())
    );
    assert!(bridge.action_envelope_is_current(direct.action.id).await);
    assert!(
        bridge.take_public_actions(session_id, 8).await.is_empty(),
        "direct consequential actions must never become visible to the legacy V1-V3 public executor queue"
    );
}
