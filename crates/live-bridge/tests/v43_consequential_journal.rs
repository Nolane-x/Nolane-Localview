use std::{fs::OpenOptions, io::Write, path::PathBuf};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, ConsequentialRecoveryState,
    DispatchLinearizationReceipt,
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
async fn crash_after_dispatch_replays_as_possibly_dispatched_and_requires_reconciliation() {
    let path = journal_path("possibly-dispatched");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    journal.record_intent_admitted(action.clone()).await.unwrap();
    journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            false,
        )
        .await
        .unwrap();
    journal
        .record_dispatch_linearized(
            action.transport_action_id,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
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
    journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            false,
        )
        .await
        .unwrap();
    journal
        .record_dispatch_linearized(
            action.transport_action_id,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
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
    journal
        .record_dispatch_linearized(
            action.transport_action_id,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
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
    assert_eq!(entries.len(), 4);
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
