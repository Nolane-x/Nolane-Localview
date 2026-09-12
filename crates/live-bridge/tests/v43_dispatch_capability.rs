use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, ConsequentialRecoveryState,
    DispatchLinearizationReceipt, DispatchPreparationReceipt,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
    TransportResult,
};
use uuid::Uuid;

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

fn envelope() -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner:capability"),
            acting_principal_ref: PrincipalRef::from("principal:executor:capability"),
            authorization_revision: "auth:capability:v1".into(),
            precondition_snapshot_cut_ref: "cut:capability:1".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:capability:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:capability:1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:capability".into()],
        },
    }
}

fn preparation(
    action: &CanonicalActionEnvelope,
    authorization_journal_sequence: u64,
) -> DispatchPreparationReceipt {
    DispatchPreparationReceipt {
        receipt_ref: format!("prepared:capability:{authorization_journal_sequence}"),
        authorization_journal_sequence,
        precondition_snapshot_cut_ref: action.metadata.precondition_snapshot_cut_ref.clone(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
    }
}

fn dispatch_receipt() -> DispatchLinearizationReceipt {
    DispatchLinearizationReceipt {
        receipt_ref: "dispatch:capability:1".into(),
        transport_result: TransportResult::DeliveredToExecutor,
        dispatch_result: DispatchResult::DispatchedFull,
    }
}

async fn admitted_and_authorized(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) -> u64 {
    journal.record_intent_admitted(action.clone()).await.unwrap();
    journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap()
        .journal_sequence
}

#[tokio::test]
async fn exact_prepared_capability_is_required_before_dispatch_can_begin() {
    let path = journal_path("one-shot-capability");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let authorization_sequence = admitted_and_authorized(&journal, &action).await;

    let admission = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation(&action, authorization_sequence),
        )
        .await
        .unwrap();
    assert!(admission.entry().journal_sequence > authorization_sequence);

    let (prepared_entry, capability) = admission.into_parts();
    let permit = journal.begin_dispatch(capability).await.unwrap();
    assert_eq!(permit.action_id(), action.transport_action_id);
    assert_eq!(
        permit.preparation_journal_sequence(),
        prepared_entry.journal_sequence
    );
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );

    journal
        .record_dispatch_linearized(permit, dispatch_receipt())
        .await
        .unwrap();
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn prepared_capability_from_a_prior_journal_instance_cannot_resume_after_reopen() {
    let path = journal_path("capability-reopen");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let authorization_sequence = admitted_and_authorized(&journal, &action).await;
    let admission = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation(&action, authorization_sequence),
        )
        .await
        .unwrap();
    let (_, capability) = admission.into_parts();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let error = reopened.begin_dispatch(capability).await.unwrap_err();
    assert!(matches!(
        error,
        ConsequentialJournalError::InvalidDispatchCapability { action_id, .. }
            if action_id == action.transport_action_id
    ));
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(
        reopened.requires_reconciliation(action.transport_action_id).await,
        Some(true)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn abandoning_an_execution_permit_never_restores_retry_authority() {
    let path = journal_path("capability-abandon");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let authorization_sequence = admitted_and_authorized(&journal, &action).await;
    let admission = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation(&action, authorization_sequence),
        )
        .await
        .unwrap();
    let (_, capability) = admission.into_parts();
    let permit = journal.begin_dispatch(capability).await.unwrap();
    assert_eq!(permit.action_id(), action.transport_action_id);
    drop(permit);

    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(
        journal.requires_reconciliation(action.transport_action_id).await,
        Some(true)
    );

    let duplicate_prepare = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation(&action, authorization_sequence),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        duplicate_prepare,
        ConsequentialJournalError::InvalidTransition { action_id, .. }
            if action_id == action.transport_action_id
    ));

    let _ = std::fs::remove_file(path);
}
