use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    BridgeActionKind, LiveBridge, ProviderObserverBatch,
};
use localview_protocol::{PrincipalRef, ProviderIncarnationRef, TargetIncarnationRef};
use uuid::Uuid;

fn metadata(
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:planner"),
        acting_principal_ref: PrincipalRef::from("principal:executor"),
        authorization_revision: "auth:v7".into(),
        precondition_snapshot_cut_ref: "cut:42".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        risk_class: ActionRiskClass::ExternalSideEffect,
        idempotency_class: ActionIdempotencyClass::NonIdempotent,
        expected_postcondition_contract_refs: vec!["postcondition:message-visible".into()],
    }
}

#[tokio::test]
async fn canonical_action_envelope_binds_principals_authority_cut_and_incarnations() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: Vec::new(),
        })
        .await;

    let queued = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(provider, target),
        )
        .await
        .unwrap();

    assert_eq!(queued.action.id, queued.envelope.transport_action_id);
    assert_eq!(queued.action.session_id, queued.envelope.session_id);
    assert_eq!(
        queued.envelope.metadata.decision_principal_ref.as_str(),
        "principal:planner"
    );
    assert_eq!(
        queued.envelope.metadata.acting_principal_ref.as_str(),
        "principal:executor"
    );
    assert_eq!(queued.envelope.metadata.authorization_revision, "auth:v7");
    assert_eq!(
        queued.envelope.metadata.precondition_snapshot_cut_ref,
        "cut:42"
    );
    assert_eq!(
        bridge.action_envelope(queued.action.id).await.unwrap(),
        queued.envelope
    );
}

#[tokio::test]
async fn canonical_action_rejects_provider_or_target_incarnation_mismatch_before_queueing() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: Vec::new(),
        })
        .await;

    let error = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(
                ProviderIncarnationRef::from("provider:webview:stale"),
                target,
            ),
        )
        .await
        .unwrap_err();

    assert_eq!(error, ActionEnvelopeBindingError::ProviderIncarnationMismatch);
    assert!(bridge.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn canonical_envelope_is_immutable_evidence_after_provider_reincarnation() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let old_provider = ProviderIncarnationRef::from("provider:webview:old");
    let target = TargetIncarnationRef::from("target:webview:1");

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: old_provider.clone(),
            target_incarnation_ref: target.clone(),
            events: Vec::new(),
        })
        .await;

    let queued = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(old_provider.clone(), target.clone()),
        )
        .await
        .unwrap();

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:webview:new"),
            target_incarnation_ref: target,
            events: Vec::new(),
        })
        .await;

    let retained = bridge.action_envelope(queued.action.id).await.unwrap();
    assert_eq!(retained.metadata.provider_incarnation_ref, old_provider);
    assert!(!bridge.action_envelope_is_current(queued.action.id).await);
}

#[tokio::test]
async fn legacy_bridge_action_remains_compact_and_has_no_canonical_authority_by_inference() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let action = bridge
        .enqueue_action(session_id, Some("@legacy".into()), BridgeActionKind::Focus)
        .await;

    let wire = serde_json::to_value(&action).unwrap();
    assert!(wire.get("decision_principal_ref").is_none());
    assert!(wire.get("acting_principal_ref").is_none());
    assert!(wire.get("authorization_revision").is_none());
    assert!(wire.get("precondition_snapshot_cut_ref").is_none());
    assert!(bridge.action_envelope(action.id).await.is_none());
}

#[test]
fn risk_and_idempotency_are_typed_not_boolean_shortcuts() {
    assert_eq!(
        serde_json::to_string(&ActionRiskClass::ObserveOnly).unwrap(),
        "\"observe_only\""
    );
    assert_eq!(
        serde_json::to_string(&ActionRiskClass::CredentialOrAuthorityChange).unwrap(),
        "\"credential_or_authority_change\""
    );
    assert_eq!(
        serde_json::to_string(&ActionIdempotencyClass::Unknown).unwrap(),
        "\"unknown\""
    );
}
