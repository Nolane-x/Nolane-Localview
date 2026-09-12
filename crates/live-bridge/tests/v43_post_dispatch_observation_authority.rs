use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialPostconditionObservationCause,
    ConsequentialPostconditionObservationError, ConsequentialRecoveryState,
    DispatchLinearizationReceipt, DispatchPreparationReceipt,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderIncarnationRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef, TransportResult,
};
use uuid::Uuid;

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

fn action() -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner:post-observe"),
            acting_principal_ref: PrincipalRef::from("principal:executor:post-observe"),
            authorization_revision: "auth:post-observe:v1".into(),
            precondition_snapshot_cut_ref: "cut:before-dispatch".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:post-observe:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:window:post-observe:1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["post:visible".into()],
        },
    }
}

async fn admit_and_authorize(
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

async fn prepare(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
    authorization_journal_sequence: u64,
) -> localview_live_bridge::DispatchPreparedAdmission {
    journal
        .record_dispatch_prepared(
            action.transport_action_id,
            DispatchPreparationReceipt {
                receipt_ref: format!("prepared:{authorization_journal_sequence}"),
                authorization_journal_sequence,
                precondition_snapshot_cut_ref: action.metadata.precondition_snapshot_cut_ref.clone(),
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap()
}

fn snapshot_receipt(
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    cut: &str,
) -> ReconciliationSnapshotReceipt {
    ReconciliationSnapshotReceipt {
        receipt_id: receipt_id.into(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref: cut.into(),
        surface_scope: "selected-window".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:v1".into(),
        permission_visibility_revision: "visibility:v1".into(),
        capture_sequence: 2,
        observed_digest: format!("digest:{cut}"),
        incompleteness_debt: Vec::new(),
    }
}

#[tokio::test]
async fn observation_authority_cannot_exist_before_dispatch_uncertainty() {
    let path = journal_path("post-observe-before-dispatch");
    let action = action();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal.record_intent_admitted(action.clone()).await.unwrap();

    let error = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialPostconditionObservationError::InvalidRecoveryState {
            current: Some(ConsequentialRecoveryState::Admitted),
            ..
        }
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn live_prepared_or_execution_authority_blocks_postcondition_observation() {
    let path = journal_path("post-observe-live-dispatch-authority");
    let action = action();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let auth_seq = admit_and_authorize(&journal, &action).await;
    let prepared = prepare(&journal, &action, auth_seq).await;
    let (_, capability) = prepared.into_parts();

    let error = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialPostconditionObservationError::DispatchAuthorityStillLive { .. }
    ));

    let permit = journal.begin_dispatch(capability).await.unwrap();
    let error = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialPostconditionObservationError::DispatchAuthorityStillLive { .. }
    ));

    journal
        .record_dispatch_linearized(
            permit,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:linearized:post-observe".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn linearized_dispatch_mints_exact_fresh_observation_cut_and_causal_binding() {
    let path = journal_path("post-observe-linearized");
    let action = action();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let auth_seq = admit_and_authorize(&journal, &action).await;
    let prepared = prepare(&journal, &action, auth_seq).await;
    let (_, capability) = prepared.into_parts();
    let dispatch_permit = journal.begin_dispatch(capability).await.unwrap();
    let linearized = journal
        .record_dispatch_linearized(
            dispatch_permit,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:linearized:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();

    let observation = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .expect("durably linearized dispatch must allow a fresh post-dispatch observation");
    assert_eq!(observation.action_id(), action.transport_action_id);
    assert_eq!(observation.session_id(), action.session_id);
    assert_eq!(
        observation.provider_incarnation_ref(),
        &action.metadata.provider_incarnation_ref
    );
    assert_eq!(
        observation.target_incarnation_ref(),
        &action.metadata.target_incarnation_ref
    );
    assert_ne!(
        observation.snapshot_cut_ref(),
        action.metadata.precondition_snapshot_cut_ref,
        "postcondition proof must never reuse the pre-dispatch observation cut"
    );
    assert!(matches!(
        observation.cause(),
        ConsequentialPostconditionObservationCause::DispatchLinearized {
            journal_sequence,
            receipt_ref,
        } if *journal_sequence == linearized.journal_sequence
            && receipt_ref == "dispatch:linearized:1"
    ));

    let cut = observation.snapshot_cut_ref().to_owned();
    let receipt = journal
        .complete_postcondition_observation(
            observation,
            snapshot_receipt(&action, "reconcile:after-dispatch", &cut),
        )
        .await
        .expect("only the exact freshly minted snapshot cut may complete observation");
    assert_eq!(receipt.action_id(), action.transport_action_id);
    assert_eq!(receipt.snapshot_cut_ref(), cut);
    assert_eq!(receipt.reconciliation_receipt_ref(), "reconcile:after-dispatch");
    assert_eq!(receipt.causal_journal_sequence(), linearized.journal_sequence);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn crash_reopened_prepared_state_can_observe_but_cannot_recreate_dispatch_authority() {
    let path = journal_path("post-observe-crash-prepared");
    let action = action();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let auth_seq = admit_and_authorize(&journal, &action).await;
    let prepared = prepare(&journal, &action, auth_seq).await;
    let prepared_sequence = prepared.entry().journal_sequence;
    drop(prepared);
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    let observation = reopened
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .expect("reopened PREPARED state has no live dispatch grant and must reconcile uncertainty");
    assert!(matches!(
        observation.cause(),
        ConsequentialPostconditionObservationCause::DispatchPreparedUncertain {
            journal_sequence,
            ..
        } if *journal_sequence == prepared_sequence
    ));

    let retry_authorization = reopened
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        retry_authorization,
        localview_live_bridge::ConsequentialJournalError::InvalidTransition { .. }
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn observation_completion_rejects_a_stale_or_forged_snapshot_cut() {
    let path = journal_path("post-observe-forged-cut");
    let action = action();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let auth_seq = admit_and_authorize(&journal, &action).await;
    let prepared = prepare(&journal, &action, auth_seq).await;
    let (_, capability) = prepared.into_parts();
    let dispatch_permit = journal.begin_dispatch(capability).await.unwrap();
    journal
        .record_dispatch_linearized(
            dispatch_permit,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:linearized:forged-cut".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();

    let observation = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let error = journal
        .complete_postcondition_observation(
            observation,
            snapshot_receipt(
                &action,
                "reconcile:stale",
                &action.metadata.precondition_snapshot_cut_ref,
            ),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialPostconditionObservationError::SnapshotCutMismatch { .. }
    ));

    let _ = std::fs::remove_file(path);
}
