use std::{fs::OpenOptions, io::Write, path::PathBuf};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, ConsequentialRecoveryState,
    DispatchExecutionPermit, DispatchLinearizationReceipt, DispatchPreparationReceipt,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
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
            decision_principal_ref: PrincipalRef::from("principal:planner"),
            acting_principal_ref: PrincipalRef::from("principal:executor"),
            authorization_revision: "auth:v7".into(),
            precondition_snapshot_cut_ref: "cut:42".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:webview:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:webview:1"),
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec!["postcondition:message-visible".into()],
        },
    }
}

fn preparation_receipt(
    action: &CanonicalActionEnvelope,
    authorization_journal_sequence: u64,
) -> DispatchPreparationReceipt {
    DispatchPreparationReceipt {
        receipt_ref: format!("dispatch-prepared:{authorization_journal_sequence}"),
        authorization_journal_sequence,
        precondition_snapshot_cut_ref: action.metadata.precondition_snapshot_cut_ref.clone(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
    }
}

fn dispatch_receipt() -> DispatchLinearizationReceipt {
    DispatchLinearizationReceipt {
        receipt_ref: "dispatch:1".into(),
        transport_result: TransportResult::DeliveredToExecutor,
        dispatch_result: DispatchResult::DispatchedFull,
    }
}

async fn authorize_prepare_and_begin(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) -> DispatchExecutionPermit {
    let authorized = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let admission = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation_receipt(action, authorized.journal_sequence),
        )
        .await
        .unwrap();
    let (_, capability) = admission.into_parts();
    journal.begin_dispatch(capability).await.unwrap()
}

#[tokio::test]
async fn journal_sequence_survives_reopen_and_is_the_causal_order() {
    let path = journal_path("sequence");
    let action = envelope();

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let admitted = journal.record_intent_admitted(action.clone()).await.unwrap();
    let authorized = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            false,
        )
        .await
        .unwrap();

    assert_eq!(admitted.journal_sequence, 1);
    assert_eq!(authorized.journal_sequence, 2);
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let entries = reopened.entries_for(action.transport_action_id).await;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].journal_sequence, 1);
    assert_eq!(entries[1].journal_sequence, 2);

    let revalidated = reopened
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(revalidated.journal_sequence, 3);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn crash_after_prepare_before_dispatch_receipt_replays_as_dispatch_uncertain() {
    let path = journal_path("prepared-dispatch-uncertain");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    journal.record_intent_admitted(action.clone()).await.unwrap();
    let authorized = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let prepared = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation_receipt(&action, authorized.journal_sequence),
        )
        .await
        .unwrap();
    assert!(prepared.entry().journal_sequence > authorized.journal_sequence);
    drop(prepared);
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await.unwrap(),
        ConsequentialRecoveryState::DispatchPrepared
    );
    assert!(
        reopened
            .requires_reconciliation(action.transport_action_id)
            .await
            .unwrap()
    );

    let duplicate_prepare = reopened
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation_receipt(&action, authorized.journal_sequence),
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

#[tokio::test]
async fn preparation_requires_latest_revalidated_authority_and_exact_binding() {
    let path = journal_path("prepared-binding");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal.record_intent_admitted(action.clone()).await.unwrap();

    let non_revalidated = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            false,
        )
        .await
        .unwrap();
    let error = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation_receipt(&action, non_revalidated.journal_sequence),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ConsequentialJournalError::InvalidTransition { .. }));

    let revalidated = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let mut stale = preparation_receipt(&action, non_revalidated.journal_sequence);
    let stale_error = journal
        .record_dispatch_prepared(action.transport_action_id, stale.clone())
        .await
        .unwrap_err();
    assert!(matches!(stale_error, ConsequentialJournalError::InvalidTransition { .. }));

    stale.authorization_journal_sequence = revalidated.journal_sequence;
    stale.target_incarnation_ref = TargetIncarnationRef::from("target:webview:forged");
    let forged_error = journal
        .record_dispatch_prepared(action.transport_action_id, stale)
        .await
        .unwrap_err();
    assert!(matches!(forged_error, ConsequentialJournalError::InvalidTransition { .. }));

    journal
        .record_dispatch_prepared(
            action.transport_action_id,
            preparation_receipt(&action, revalidated.journal_sequence),
        )
        .await
        .unwrap();

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn authorization_alone_remains_not_dispatched_without_prepared_capability() {
    let path = journal_path("no-prepare-bypass");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal.record_intent_admitted(action.clone()).await.unwrap();
    journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();

    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::AuthorizedNotDispatched)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn crash_after_dispatch_replays_as_possibly_dispatched_and_requires_reconciliation() {
    let path = journal_path("possibly-dispatched");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    journal.record_intent_admitted(action.clone()).await.unwrap();
    let permit = authorize_prepare_and_begin(&journal, &action).await;
    journal
        .record_dispatch_linearized(permit, dispatch_receipt())
        .await
        .unwrap();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await.unwrap(),
        ConsequentialRecoveryState::PossiblyDispatched
    );
    assert!(
        reopened
            .requires_reconciliation(action.transport_action_id)
            .await
            .unwrap()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn verified_outcome_remains_uncommitted_until_commit_is_durable() {
    let path = journal_path("verified-uncommitted");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    journal.record_intent_admitted(action.clone()).await.unwrap();
    let permit = authorize_prepare_and_begin(&journal, &action).await;
    journal
        .record_dispatch_linearized(permit, dispatch_receipt())
        .await
        .unwrap();
    journal
        .record_reconciliation_outcome(
            action.transport_action_id,
            WorldOutcome::VerifiedExpected,
            Some("reconcile:1".into()),
            vec!["postcondition:message-visible:receipt".into()],
            true,
        )
        .await
        .unwrap();

    assert_eq!(
        journal.recovery_state(action.transport_action_id).await.unwrap(),
        ConsequentialRecoveryState::VerifiedUncommitted
    );

    journal
        .record_committed(action.transport_action_id)
        .await
        .unwrap();
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await.unwrap(),
        ConsequentialRecoveryState::Committed
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn compensation_is_additive_history_not_rewrite_of_prior_effect() {
    let path = journal_path("compensation");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    journal.record_intent_admitted(action.clone()).await.unwrap();
    let permit = authorize_prepare_and_begin(&journal, &action).await;
    journal
        .record_dispatch_linearized(permit, dispatch_receipt())
        .await
        .unwrap();
    journal
        .record_reconciliation_outcome(
            action.transport_action_id,
            WorldOutcome::VerifiedUnexpected,
            Some("reconcile:unexpected".into()),
            Vec::new(),
            false,
        )
        .await
        .unwrap();
    journal
        .record_compensation(
            action.transport_action_id,
            "compensation:undo-1".into(),
            WorldOutcome::CompensatedVerified,
        )
        .await
        .unwrap();

    let entries = journal.entries_for(action.transport_action_id).await;
    assert_eq!(entries.len(), 6);
    assert!(entries
        .windows(2)
        .all(|pair| pair[0].journal_sequence < pair[1].journal_sequence));
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await.unwrap(),
        ConsequentialRecoveryState::Compensated
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn invalid_lifecycle_transitions_are_typed_and_never_appended() {
    let path = journal_path("invalid-transition");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    let unknown = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        unknown,
        ConsequentialJournalError::UnknownAction { action_id }
            if action_id == action.transport_action_id
    ));

    journal.record_intent_admitted(action.clone()).await.unwrap();
    let invalid_commit = journal
        .record_committed(action.transport_action_id)
        .await
        .unwrap_err();
    assert!(matches!(
        invalid_commit,
        ConsequentialJournalError::InvalidTransition { action_id, .. }
            if action_id == action.transport_action_id
    ));
    assert_eq!(journal.entries_for(action.transport_action_id).await.len(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn incomplete_trailing_record_is_discarded_before_new_durable_append() {
    let path = journal_path("truncated-tail");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal.record_intent_admitted(action.clone()).await.unwrap();
    drop(journal);

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"schema_version\":1,\"journal_sequence\":2")
        .unwrap();
    file.flush().unwrap();
    drop(file);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.entries_for(action.transport_action_id).await.len(),
        1
    );
    let second = reopened
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(second.journal_sequence, 2);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn complete_corrupt_record_is_not_silently_ignored() {
    let path = journal_path("corrupt-record");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal.record_intent_admitted(action).await.unwrap();
    drop(journal);

    let mut file = OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"not-json\n").unwrap();
    file.flush().unwrap();
    drop(file);

    let error = ConsequentialJournal::open(&path).await.unwrap_err();
    assert!(matches!(
        error,
        ConsequentialJournalError::CorruptRecord { line: 2, .. }
    ));

    let _ = std::fs::remove_file(path);
}
