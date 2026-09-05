use std::path::PathBuf;

use localview_live_bridge::{
    reconcile_consequential_postconditions, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalError,
    ConsequentialPostconditionEvidence, ConsequentialPostconditionReconciliationReceipt,
    ConsequentialPostconditionStatus, ConsequentialReconciliationError,
    ConsequentialRecoveryState, DispatchLinearizationReceipt, DispatchPreparationReceipt,
    LiveBridge, ProviderObservationBinding,
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

fn action(expected: &[&str]) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner:recovery"),
            acting_principal_ref: PrincipalRef::from("principal:executor:recovery"),
            authorization_revision: "auth:recovery:v1".into(),
            precondition_snapshot_cut_ref: "cut:before:recovery".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:recovery:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:window:recovery:1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: expected.iter().map(|value| (*value).into()).collect(),
        },
    }
}

async fn linearize(journal: &ConsequentialJournal, action: &CanonicalActionEnvelope) {
    journal.record_intent_admitted(action.clone()).await.unwrap();
    let authorization = journal
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
            DispatchPreparationReceipt {
                receipt_ref: format!("prepared:{}", authorization.journal_sequence),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: action.metadata.precondition_snapshot_cut_ref.clone(),
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    let (_, capability) = prepared.into_parts();
    let permit = journal.begin_dispatch(capability).await.unwrap();
    journal
        .record_dispatch_linearized(
            permit,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:recovery:1".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();
}

async fn bind_snapshot(
    bridge: &LiveBridge,
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    cut: &str,
    completeness: ReconciliationCompleteness,
    debt: Vec<String>,
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
                    snapshot_cut_ref: cut.into(),
                    surface_scope: "selected-window".into(),
                    completeness,
                    cache_profile_revision: "cache:v1".into(),
                    permission_visibility_revision: "visibility:v1".into(),
                    capture_sequence: 7,
                    observed_digest: format!("digest:{cut}"),
                    incompleteness_debt: debt,
                },
            )
            .await
    );
}

fn reconciliation(
    action: &CanonicalActionEnvelope,
    receipt_id: &str,
    cut: &str,
    postconditions: Vec<ConsequentialPostconditionEvidence>,
) -> ConsequentialPostconditionReconciliationReceipt {
    ConsequentialPostconditionReconciliationReceipt {
        action_id: action.transport_action_id,
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref: cut.into(),
        reconciliation_receipt_ref: receipt_id.into(),
        postconditions,
    }
}

fn evidence(
    contract_ref: &str,
    status: ConsequentialPostconditionStatus,
    receipt_ref: &str,
) -> ConsequentialPostconditionEvidence {
    ConsequentialPostconditionEvidence {
        contract_ref: contract_ref.into(),
        status,
        receipt_ref: receipt_ref.into(),
    }
}

#[tokio::test]
async fn incomplete_reconciliation_survives_restart_and_later_exact_evidence_closes_it() {
    let path = journal_path("postcondition-restart");
    let action = action(&["post:visible", "post:enabled"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize(&journal, &action).await;
    bind_snapshot(
        &bridge,
        &action,
        "reconcile:before-restart",
        "cut:after:1",
        ReconciliationCompleteness::Established,
        Vec::new(),
    )
    .await;

    let first = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        reconciliation(
            &action,
            "reconcile:before-restart",
            "cut:after:1",
            vec![evidence(
                "post:visible",
                ConsequentialPostconditionStatus::VerifiedPass,
                "post-receipt:visible:1",
            )],
        ),
    )
    .await
    .unwrap();
    assert_eq!(first.world_outcome, WorldOutcome::ReconciliationRequired);
    drop(journal);
    drop(bridge);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert_eq!(
        reopened.requires_reconciliation(action.transport_action_id).await,
        Some(true)
    );

    let rebound = LiveBridge::new(32, 8);
    bind_snapshot(
        &rebound,
        &action,
        "reconcile:after-restart",
        "cut:after:2",
        ReconciliationCompleteness::Established,
        Vec::new(),
    )
    .await;
    let closed = reconcile_consequential_postconditions(
        &rebound,
        &reopened,
        reconciliation(
            &action,
            "reconcile:after-restart",
            "cut:after:2",
            vec![
                evidence(
                    "post:visible",
                    ConsequentialPostconditionStatus::VerifiedPass,
                    "post-receipt:visible:2",
                ),
                evidence(
                    "post:enabled",
                    ConsequentialPostconditionStatus::VerifiedPass,
                    "post-receipt:enabled:2",
                ),
            ],
        ),
    )
    .await
    .expect("durable unverified outcome must remain reconcilable after restart");
    assert_eq!(closed.world_outcome, WorldOutcome::VerifiedExpected);
    assert!(closed.postconditions_verified);
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn incomplete_snapshot_cannot_be_promoted_to_verified_postcondition() {
    let path = journal_path("postcondition-incomplete-snapshot");
    let action = action(&["post:visible"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize(&journal, &action).await;
    bind_snapshot(
        &bridge,
        &action,
        "reconcile:incomplete",
        "cut:after:incomplete",
        ReconciliationCompleteness::Incomplete,
        vec!["uia:node-budget-exhausted".into()],
    )
    .await;
    let before = journal.entries_for(action.transport_action_id).await.len();

    let error = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        reconciliation(
            &action,
            "reconcile:incomplete",
            "cut:after:incomplete",
            vec![evidence(
                "post:visible",
                ConsequentialPostconditionStatus::VerifiedPass,
                "post-receipt:must-not-promote",
            )],
        ),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ConsequentialReconciliationError::ReconciliationSnapshotIncomplete
    ));
    assert_eq!(
        journal.entries_for(action.transport_action_id).await.len(),
        before,
        "incomplete observation evidence must append no verified outcome"
    );
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unknown_postcondition_preserves_uncertainty_and_never_reopens_dispatch_authority() {
    let path = journal_path("postcondition-unknown");
    let action = action(&["post:visible"]);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize(&journal, &action).await;
    bind_snapshot(
        &bridge,
        &action,
        "reconcile:unknown",
        "cut:after:unknown",
        ReconciliationCompleteness::Established,
        Vec::new(),
    )
    .await;

    let result = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        reconciliation(
            &action,
            "reconcile:unknown",
            "cut:after:unknown",
            vec![evidence(
                "post:visible",
                ConsequentialPostconditionStatus::Unknown,
                "post-receipt:unknown",
            )],
        ),
    )
    .await
    .unwrap();
    assert_eq!(result.world_outcome, WorldOutcome::ReconciliationRequired);
    assert!(!result.postconditions_verified);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );

    let retry_authorization = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        retry_authorization,
        ConsequentialJournalError::InvalidTransition { .. }
    ));

    let _ = std::fs::remove_file(path);
}
