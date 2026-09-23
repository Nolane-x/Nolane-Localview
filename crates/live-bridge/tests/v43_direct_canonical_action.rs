use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BoundCanonicalDispatchError,
    BridgeActionKind, LiveBridge, ProviderObserverBatch,
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


#[tokio::test]
async fn direct_canonical_action_enters_public_queue_once_with_exact_transport_id() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::from_u128(0x9201);
    let provider = ProviderIncarnationRef::from("provider:managed-webview:r4");
    let target = TargetIncarnationRef::from("target:managed-webview:r4");
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
            Some("@eabc123".into()),
            BridgeActionKind::Click,
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from(
                    "principal:local-control:bearer-holder-explicit-confirmation-v1",
                ),
                acting_principal_ref: PrincipalRef::from(
                    "principal:localview-daemon:managed-webview-v1",
                ),
                authorization_revision: "authorization:r4:1".into(),
                precondition_snapshot_cut_ref: "cut:r4:1".into(),
                provider_incarnation_ref: provider,
                target_incarnation_ref: target,
                risk_class: ActionRiskClass::DestructiveOrIrreversible,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![
                    "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edone\"}"
                        .into(),
                ],
            },
        )
        .await
        .unwrap();
    let replay = direct.clone();
    let action_id = direct.action.id;

    bridge
        .enqueue_bound_canonical_action_for_dispatch(direct)
        .await
        .expect("one-shot canonical dispatch handoff should succeed");

    assert!(
        bridge.action_envelope(action_id).await.is_none(),
        "queue handoff must consume the in-memory direct binding"
    );
    let drained = bridge.take_public_actions(session_id, 8).await;
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, action_id);
    assert_eq!(drained[0].reference.as_deref(), Some("@eabc123"));

    assert_eq!(
        bridge
            .enqueue_bound_canonical_action_for_dispatch(replay)
            .await,
        Err(BoundCanonicalDispatchError::MissingCanonicalEnvelope),
        "the same confirmed canonical action must never be queued twice"
    );
}

#[tokio::test]
async fn stale_direct_canonical_dispatch_consumes_one_shot_binding_without_queueing() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::from_u128(0x9202);
    let provider = ProviderIncarnationRef::from("provider:managed-webview:r4:old");
    let target = TargetIncarnationRef::from("target:managed-webview:r4");
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
            Some("@eabc124".into()),
            BridgeActionKind::Click,
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from(
                    "principal:local-control:bearer-holder-explicit-confirmation-v1",
                ),
                acting_principal_ref: PrincipalRef::from(
                    "principal:localview-daemon:managed-webview-v1",
                ),
                authorization_revision: "authorization:r4:2".into(),
                precondition_snapshot_cut_ref: "cut:r4:2".into(),
                provider_incarnation_ref: provider,
                target_incarnation_ref: target.clone(),
                risk_class: ActionRiskClass::DestructiveOrIrreversible,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![
                    "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edone\"}"
                        .into(),
                ],
            },
        )
        .await
        .unwrap();
    let action_id = direct.action.id;

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 2,
            provider_incarnation_ref: ProviderIncarnationRef::from(
                "provider:managed-webview:r4:new",
            ),
            target_incarnation_ref: target,
            events: Vec::new(),
        })
        .await;

    assert_eq!(
        bridge
            .enqueue_bound_canonical_action_for_dispatch(direct)
            .await,
        Err(BoundCanonicalDispatchError::ProviderIncarnationMismatch)
    );
    assert!(bridge.action_envelope(action_id).await.is_none());
    assert!(bridge.take_public_actions(session_id, 8).await.is_empty());
}
