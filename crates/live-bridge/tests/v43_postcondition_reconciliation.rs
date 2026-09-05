use std::path::PathBuf;

use localview_live_bridge::{
    reconcile_consequential_postconditions, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalError,
    ConsequentialPostconditionEvidence, ConsequentialPostconditionStatus,
    ConsequentialPostconditionReconciliationReceipt, ConsequentialReconciliationError,
    ConsequentialRecoveryState, DispatchExecutionPermit, DispatchLinearizationReceipt,
    DispatchPreparationReceipt, LiveBridge, ProviderObservationBinding,
};
use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderIncarnationRef,
    ReconciliationCompleteness, ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
use uuid::Uuid;

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

fn envelope(expected: &[&str]) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner"),
            acting_principal_ref: PrincipalRef::from("principal:executor"),
            authorization_revision: "auth:postcondition:v1".into(),
            precondition_snapshot_cut_ref: "cut:before".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:window:1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: expected.iter().map(|value| (*value).into()).collect(),
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

async fn dispatch_once(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) -> DispatchExecutionPermit {
    journal.record_intent_admitted(action.clone()).await.unwrap();
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

async fn linearize_dispatch(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) {
    let permit = dispatch_once(journal, action).await;
    journal
        .record_dispatch_linearized(
            permit,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:linearized:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();
}

async fn bind_reconciliation_snapshot(
    bridge: &LiveBridge,
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    snapshot_cut_ref: &str,
) {
    if bridge.observation_status(action.session_id).await.is_none() {
        bridge
            .bind_provider_observation(ProviderObservationBinding {
                session_id: action.session_id,
                generation: 1,
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
                initial_continuity: EventContinuityState::OrderingOpaque,
                sequence_baseline: Some(0),
            })
            .await
            .unwrap();
    }
    assert!(
        bridge
            .record_reconciliation(
                action.session_id,
                ReconciliationSnapshotReceipt {
                    receipt_id: receipt_id.into(),
                    provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                    target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
                    snapshot_cut_ref: snapshot_cut_ref.into(),
                    surface_scope: "selected-window".into(),
                    completeness: ReconciliationCompleteness::Established,
                    cache_profile_revision: "cache:v1".into(),
                    permission_visibility_revision: "visibility:v1".into(),
                    capture_sequence: 2,
                    observed_digest: format!("digest:{snapshot_cut_ref}"),
                    incompleteness_debt: Vec::new(),
                },
            )
            .await
    );
}

fn evidence(
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    snapshot_cut_ref: &str,
    postconditions: Vec<ConsequentialPostconditionEvidence>,
) -> ConsequentialPostconditionReconciliationReceipt {
    ConsequentialPostconditionReconciliationReceipt {
        action_id: action.transport_action_id,
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref: snapshot_cut_ref.into(),
        reconciliation_receipt_ref: receipt_id.into(),
        postconditions,
    }
}

fn pass(contract_ref: &str, receipt_ref: &str) -> ConsequentialPostconditionEvidence {
    ConsequentialPostconditionEvidence {
        contract_ref: contract_ref.into(),
        status: ConsequentialPostconditionStatus::VerifiedPass,
        receipt_ref: receipt_ref.into(),
    }
}

#[tokio::test]
async fn only_complete_exact_fresh_postcondition_set_can_reach_verified_uncommitted() {
    let path = journal_path("typed-postcondition-pass");
    let action = envelope(&["post:visible", "post:enabled"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize_dispatch(&journal, &action).await;
    bind_reconciliation_snapshot(&bridge, &action, "reconcile:1", "cut:after:1").await;

    let result = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        evidence(
            &action,
            "reconcile:1",
            "cut:after:1",
            vec![pass("post:visible", "post-receipt:1"), pass("post:enabled", "post-receipt:2")],
        ),
    )
    .await
    .unwrap();

    assert_eq!(result.world_outcome, WorldOutcome::VerifiedExpected);
    assert!(result.postconditions_verified);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );
    assert!(!journal
        .requires_reconciliation(action.transport_action_id)
        .await
        .unwrap());

    journal
        .record_committed(action.transport_action_id)
        .await
        .expect("commit may happen only after typed postcondition verification");
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn incomplete_reconciliation_stays_fail_closed_and_can_be_reconciled_again() {
    let path = journal_path("typed-postcondition-repeat");
    let action = envelope(&["post:visible", "post:enabled"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize_dispatch(&journal, &action).await;
    bind_reconciliation_snapshot(&bridge, &action, "reconcile:1", "cut:after:1").await;

    let first = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        evidence(
            &action,
            "reconcile:1",
            "cut:after:1",
            vec![pass("post:visible", "post-receipt:1")],
        ),
    )
    .await
    .unwrap();
    assert_eq!(first.world_outcome, WorldOutcome::ReconciliationRequired);
    assert!(!first.postconditions_verified);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert!(journal
        .requires_reconciliation(action.transport_action_id)
        .await
        .unwrap());

    bind_reconciliation_snapshot(&bridge, &action, "reconcile:2", "cut:after:2").await;
    let second = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        evidence(
            &action,
            "reconcile:2",
            "cut:after:2",
            vec![pass("post:visible", "post-receipt:2"), pass("post:enabled", "post-receipt:3")],
        ),
    )
    .await
    .expect("unknown/incomplete evidence must never permanently block later reconciliation");
    assert_eq!(second.world_outcome, WorldOutcome::VerifiedExpected);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn forged_lineage_or_unbound_snapshot_receipt_cannot_mutate_recovery_state() {
    let path = journal_path("typed-postcondition-forgery");
    let action = envelope(&["post:visible"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize_dispatch(&journal, &action).await;
    bind_reconciliation_snapshot(&bridge, &action, "reconcile:good", "cut:after:good").await;
    let before = journal.entries_for(action.transport_action_id).await.len();

    let mut forged = evidence(
        &action,
        "reconcile:good",
        "cut:after:good",
        vec![pass("post:visible", "post-receipt:forged")],
    );
    forged.target_incarnation_ref = TargetIncarnationRef::from("target:forged");
    let error = reconcile_consequential_postconditions(&bridge, &journal, forged)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialReconciliationError::TargetIncarnationMismatch
    ));

    let unbound_snapshot = evidence(
        &action,
        "reconcile:not-recorded",
        "cut:after:forged",
        vec![pass("post:visible", "post-receipt:forged-2")],
    );
    let error = reconcile_consequential_postconditions(&bridge, &journal, unbound_snapshot)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialReconciliationError::ReconciliationSnapshotMismatch
    ));

    assert_eq!(
        journal.entries_for(action.transport_action_id).await.len(),
        before,
        "forged reconciliation evidence must not append any durable outcome"
    );
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn verified_failure_never_launders_into_commit() {
    let path = journal_path("typed-postcondition-fail");
    let action = envelope(&["post:visible"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize_dispatch(&journal, &action).await;
    bind_reconciliation_snapshot(&bridge, &action, "reconcile:fail", "cut:after:fail").await;

    let result = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        evidence(
            &action,
            "reconcile:fail",
            "cut:after:fail",
            vec![ConsequentialPostconditionEvidence {
                contract_ref: "post:visible".into(),
                status: ConsequentialPostconditionStatus::VerifiedFail,
                receipt_ref: "post-receipt:fail".into(),
            }],
        ),
    )
    .await
    .unwrap();
    assert_eq!(result.world_outcome, WorldOutcome::VerifiedUnexpected);
    assert!(!result.postconditions_verified);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    let commit_error = journal
        .record_committed(action.transport_action_id)
        .await
        .unwrap_err();
    assert!(matches!(
        commit_error,
        ConsequentialJournalError::InvalidTransition { .. }
    ));

    let _ = std::fs::remove_file(path);
}
