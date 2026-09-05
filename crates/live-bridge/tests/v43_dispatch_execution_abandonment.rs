use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, ConsequentialRecoveryState,
    DispatchPreparationReceipt,
};
use localview_protocol::{PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use uuid::Uuid;

fn envelope() -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:abandonment"),
            acting_principal_ref: PrincipalRef::from("principal:acting:abandonment"),
            authorization_revision: "authorization:abandonment:v1".into(),
            precondition_snapshot_cut_ref: "cut:pre-dispatch:abandonment".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:abandonment:v1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:abandonment:v1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:abandonment".into()],
        },
    }
}

#[tokio::test]
async fn abandoning_one_live_execution_permit_releases_only_execution_authority() {
    let path = std::env::temp_dir().join(format!(
        "localview-v43-dispatch-execution-abandonment-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let action = envelope();
    let action_id = action.transport_action_id;

    journal
        .record_intent_admitted(action.clone())
        .await
        .unwrap();
    let authorization = journal
        .record_authorization(
            action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let prepared = journal
        .record_dispatch_prepared(
            action_id,
            DispatchPreparationReceipt {
                receipt_ref: "prepared:abandonment".into(),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: action
                    .metadata
                    .precondition_snapshot_cut_ref
                    .clone(),
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    let (_, capability) = prepared.into_parts();
    let permit = journal.begin_dispatch(capability).await.unwrap();

    journal
        .abandon_dispatch_execution(permit)
        .await
        .expect("provider/receipt failure must consume the exact live execution grant");

    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared),
        "abandonment must preserve the durable uncertain PREPARED state"
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));

    let observation = journal
        .begin_postcondition_observation(action_id)
        .await
        .expect("same-process recovery must be able to reconcile after execution abandonment");
    journal
        .abandon_postcondition_observation(observation)
        .await
        .unwrap();

    let retry = journal
        .record_authorization(
            action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        retry,
        ConsequentialJournalError::InvalidTransition { .. }
    ));

    let _ = std::fs::remove_file(path);
}
