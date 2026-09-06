use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, ConsequentialJournal, ConsequentialJournalTransition, LiveBridge,
    ProviderObservationBinding,
};
use localview_protocol::{
    EventContinuityState, PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use uuid::Uuid;

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

fn metadata(provider: ProviderIncarnationRef, target: TargetIncarnationRef) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:operation-binding:decision"),
        acting_principal_ref: PrincipalRef::from("principal:operation-binding:acting"),
        authorization_revision: "authorization:operation-binding:v1".into(),
        precondition_snapshot_cut_ref: "cut:operation-binding:1".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:operation-binding".into()],
    }
}

async fn bridge_with_binding() -> (
    LiveBridge,
    SessionId,
    ProviderIncarnationRef,
    TargetIncarnationRef,
) {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::from_u128(0x8901);
    let provider = ProviderIncarnationRef::from("provider:windows-uia:operation-binding");
    let target = TargetIncarnationRef::from("target:windows:operation-binding");
    bridge
        .bind_provider_observation(ProviderObservationBinding {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            initial_continuity: EventContinuityState::OrderingOpaque,
            sequence_baseline: Some(0),
        })
        .await
        .unwrap();
    (bridge, session_id, provider, target)
}

#[tokio::test]
async fn canonical_action_retains_only_payload_free_operation_identity() {
    let (bridge, session_id, provider, target) = bridge_with_binding().await;
    let queued = bridge
        .enqueue_canonical_action(
            session_id,
            None,
            BridgeActionKind::TypeText {
                text: "secret-value-must-not-enter-operation-binding".into(),
                clear_first: true,
            },
            metadata(provider, target),
        )
        .await
        .unwrap();

    assert_eq!(
        bridge.action_operation(queued.action.id).await,
        Some(CanonicalActionOperation::InputText)
    );
    let encoded = serde_json::to_string(
        &bridge
            .action_operation(queued.action.id)
            .await
            .expect("canonical operation must exist"),
    )
    .unwrap();
    assert_eq!(encoded, "\"input_text\"");
    assert!(!encoded.contains("secret-value"));
}

#[tokio::test]
async fn canonical_operation_binding_is_durable_and_one_shot_before_authorization() {
    let (bridge, session_id, provider, target) = bridge_with_binding().await;
    let queued = bridge
        .enqueue_canonical_action(
            session_id,
            None,
            BridgeActionKind::Click,
            metadata(provider, target),
        )
        .await
        .unwrap();
    assert_eq!(
        bridge.action_operation(queued.action.id).await,
        Some(CanonicalActionOperation::Activate)
    );

    let path = journal_path("canonical-operation-binding");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    let bound = journal
        .record_intent_operation_bound(queued.action.id, CanonicalActionOperation::Activate)
        .await
        .unwrap();
    assert!(matches!(
        bound.transition,
        ConsequentialJournalTransition::IntentOperationBound {
            operation: CanonicalActionOperation::Activate
        }
    ));
    assert_eq!(
        journal.admitted_operation(queued.action.id).await,
        Some(CanonicalActionOperation::Activate)
    );
    assert!(
        journal
            .record_intent_operation_bound(queued.action.id, CanonicalActionOperation::Focus)
            .await
            .is_err(),
        "operation binding must be immutable once durably recorded"
    );
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.admitted_operation(queued.action.id).await,
        Some(CanonicalActionOperation::Activate),
        "operation identity must survive restart without reconstructing raw action payload"
    );

    let _ = std::fs::remove_file(path);
}
