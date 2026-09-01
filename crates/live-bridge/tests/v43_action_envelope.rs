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
        idempotency_class: ActionIdempotencyClass::Irreversible,
        expected_postcondition_contract_refs: vec!["postcondition:message-visible".into()],
    }
}

async fn bind_provider(
    bridge: &LiveBridge,
    session_id: Uuid,
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
) {
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            events: Vec::new(),
        })
        .await;
}

#[tokio::test]
async fn canonical_action_envelope_binds_principals_authority_cut_and_incarnations() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");

    bind_provider(&bridge, session_id, provider.clone(), target.clone()).await;

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

    bind_provider(&bridge, session_id, provider.clone(), target.clone()).await;

    let provider_error = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(
                ProviderIncarnationRef::from("provider:webview:stale"),
                target.clone(),
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(
        provider_error,
        ActionEnvelopeBindingError::ProviderIncarnationMismatch
    );

    let target_error = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(
                provider,
                TargetIncarnationRef::from("target:webview:stale"),
            ),
        )
        .await
        .unwrap_err();
    assert_eq!(
        target_error,
        ActionEnvelopeBindingError::TargetIncarnationMismatch
    );
    assert!(bridge.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn canonical_action_requires_provider_observation_before_admission() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let error = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            metadata(
                ProviderIncarnationRef::from("provider:webview:1"),
                TargetIncarnationRef::from("target:webview:1"),
            ),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ActionEnvelopeBindingError::MissingProviderObservation
    );
    assert!(bridge.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn consequential_action_requires_expected_postcondition_contract() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");
    bind_provider(&bridge, session_id, provider.clone(), target.clone()).await;

    let mut envelope = metadata(provider, target);
    envelope.expected_postcondition_contract_refs.clear();
    let error = bridge
        .enqueue_canonical_action(
            session_id,
            Some("@send".into()),
            BridgeActionKind::Click,
            envelope,
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ActionEnvelopeBindingError::MissingExpectedPostcondition
    );
    assert!(bridge.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn internal_capture_action_cannot_borrow_public_canonical_authority() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");
    bind_provider(&bridge, session_id, provider.clone(), target.clone()).await;

    let error = bridge
        .enqueue_canonical_action(
            session_id,
            None,
            BridgeActionKind::FreezeVisuals,
            metadata(provider, target),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ActionEnvelopeBindingError::InternalCaptureActionUnsupported
    );
}

#[tokio::test]
async fn empty_principal_authorization_or_cut_is_rejected_before_queueing() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");
    bind_provider(&bridge, session_id, provider.clone(), target.clone()).await;

    let mut missing_decision = metadata(provider.clone(), target.clone());
    missing_decision.decision_principal_ref = PrincipalRef::from(" ");
    assert_eq!(
        bridge
            .enqueue_canonical_action(
                session_id,
                Some("@send".into()),
                BridgeActionKind::Click,
                missing_decision,
            )
            .await
            .unwrap_err(),
        ActionEnvelopeBindingError::MissingDecisionPrincipal
    );

    let mut missing_acting = metadata(provider.clone(), target.clone());
    missing_acting.acting_principal_ref = PrincipalRef::from("");
    assert_eq!(
        bridge
            .enqueue_canonical_action(
                session_id,
                Some("@send".into()),
                BridgeActionKind::Click,
                missing_acting,
            )
            .await
            .unwrap_err(),
        ActionEnvelopeBindingError::MissingActingPrincipal
    );

    let mut missing_auth = metadata(provider.clone(), target.clone());
    missing_auth.authorization_revision = "  ".into();
    assert_eq!(
        bridge
            .enqueue_canonical_action(
                session_id,
                Some("@send".into()),
                BridgeActionKind::Click,
                missing_auth,
            )
            .await
            .unwrap_err(),
        ActionEnvelopeBindingError::MissingAuthorizationRevision
    );

    let mut missing_cut = metadata(provider, target);
    missing_cut.precondition_snapshot_cut_ref.clear();
    assert_eq!(
        bridge
            .enqueue_canonical_action(
                session_id,
                Some("@send".into()),
                BridgeActionKind::Click,
                missing_cut,
            )
            .await
            .unwrap_err(),
        ActionEnvelopeBindingError::MissingPreconditionSnapshotCut
    );

    assert!(bridge.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn canonical_envelope_is_immutable_evidence_after_provider_reincarnation() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let old_provider = ProviderIncarnationRef::from("provider:webview:old");
    let target = TargetIncarnationRef::from("target:webview:1");

    bind_provider(
        &bridge,
        session_id,
        old_provider.clone(),
        target.clone(),
    )
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
fn risk_and_idempotency_preserve_the_v4_taxonomy_on_the_wire() {
    assert_eq!(
        serde_json::to_string(&ActionRiskClass::ObserveOnly).unwrap(),
        "\"s0_observe_only\""
    );
    assert_eq!(
        serde_json::to_string(&ActionRiskClass::CredentialOrAuthorityChange).unwrap(),
        "\"s5_credential_or_authority_change\""
    );
    assert_eq!(
        serde_json::to_string(&ActionRiskClass::Unknown).unwrap(),
        "\"side_effect_unknown\""
    );

    let idempotency = [
        (ActionIdempotencyClass::PureRead, "pure_read"),
        (
            ActionIdempotencyClass::IdempotentWriteWithKey,
            "idempotent_write_with_key",
        ),
        (
            ActionIdempotencyClass::IdempotentByObservedState,
            "idempotent_by_observed_state",
        ),
        (
            ActionIdempotencyClass::CompensatableNonIdempotent,
            "compensatable_non_idempotent",
        ),
        (ActionIdempotencyClass::Irreversible, "irreversible"),
        (ActionIdempotencyClass::Unknown, "idempotency_unknown"),
    ];
    for (class, expected) in idempotency {
        assert_eq!(
            serde_json::to_string(&class).unwrap(),
            format!("\"{expected}\"")
        );
    }
}
