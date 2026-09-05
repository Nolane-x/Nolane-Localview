use std::path::PathBuf;

use localview_live_bridge::{
    reconcile_consequential_postconditions, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionPostconditionVerdict, ActionRiskClass, CanonicalActionEnvelope, ConsequentialJournal,
    ConsequentialJournalTransition, ConsequentialPostconditionEvidence,
    ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,
    ConsequentialRecoveryState, DispatchLinearizationReceipt, DispatchPreparationReceipt,
    LiveBridge, ProviderObservationBinding,
};
use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderIncarnationRef,
    ReconciliationCompleteness, ReconciliationSnapshotReceipt, TargetIncarnationRef,
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
        session_id: Uuid::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:postcondition-receipt"),
            acting_principal_ref: PrincipalRef::from("principal:acting:postcondition-receipt"),
            authorization_revision: "authorization:postcondition-receipt:v1".into(),
            precondition_snapshot_cut_ref: "cut:before:postcondition-receipt".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:postcondition-receipt"),
            target_incarnation_ref: TargetIncarnationRef::from("target:window:postcondition-receipt"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["post:visible".into(), "post:enabled".into()],
        },
    }
}

async fn linearize(journal: &ConsequentialJournal, action: &CanonicalActionEnvelope) -> u64 {
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
                receipt_ref: "dispatch:prepared:postcondition-receipt".into(),
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
                receipt_ref: "dispatch:linearized:postcondition-receipt".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap()
        .journal_sequence
}

async fn bind_and_observe(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
    reconciliation_ref: &str,
    evidence: Vec<ConsequentialPostconditionEvidence>,
) -> ConsequentialPostconditionReconciliationReceipt {
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

    let permit = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let snapshot_cut_ref = permit.snapshot_cut_ref().to_owned();
    let snapshot = ReconciliationSnapshotReceipt {
        receipt_id: reconciliation_ref.into(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref,
        surface_scope: "selected-window".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:postcondition-receipt:v1".into(),
        permission_visibility_revision: "visibility:postcondition-receipt:v1".into(),
        capture_sequence: 2,
        observed_digest: format!("digest:{reconciliation_ref}"),
        incompleteness_debt: vec![],
    };
    assert!(bridge.record_reconciliation(action.session_id, snapshot.clone()).await);
    let observation = journal
        .complete_postcondition_observation(permit, snapshot)
        .await
        .unwrap();
    ConsequentialPostconditionReconciliationReceipt::from_observation(observation, evidence)
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
async fn verified_postconditions_append_and_replay_first_class_durable_receipt() {
    let path = journal_path("action-postcondition-receipt-pass");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    let dispatch_sequence = linearize(&journal, &action).await;

    let input = bind_and_observe(
        &bridge,
        &journal,
        &action,
        "reconcile:postcondition-receipt:pass",
        vec![
            evidence(
                "post:visible",
                ConsequentialPostconditionStatus::VerifiedPass,
                "evidence:visible:pass",
            ),
            evidence(
                "post:enabled",
                ConsequentialPostconditionStatus::VerifiedPass,
                "evidence:enabled:pass",
            ),
        ],
    )
    .await;
    let result = reconcile_consequential_postconditions(&bridge, &journal, input)
        .await
        .unwrap();
    let receipt = &result.postcondition_receipt;

    assert_eq!(receipt.action_id, action.transport_action_id);
    assert_eq!(receipt.session_id, action.session_id);
    assert_eq!(
        receipt.provider_incarnation_ref,
        action.metadata.provider_incarnation_ref
    );
    assert_eq!(
        receipt.target_incarnation_ref,
        action.metadata.target_incarnation_ref
    );
    assert_eq!(
        receipt.expected_postcondition_contract_refs,
        action.metadata.expected_postcondition_contract_refs
    );
    assert_ne!(
        receipt.observation_snapshot_cut_ref,
        action.metadata.precondition_snapshot_cut_ref
    );
    assert_eq!(
        receipt.reconciliation_receipt_ref,
        "reconcile:postcondition-receipt:pass"
    );
    assert_eq!(
        receipt.evidence_receipt_refs,
        vec!["evidence:visible:pass", "evidence:enabled:pass"]
    );
    assert_eq!(receipt.verdict, ActionPostconditionVerdict::VerifiedExpected);
    assert!(receipt.unresolved_unknown_contract_refs.is_empty());
    assert_eq!(receipt.causal_assurance.causal_journal_sequence(), dispatch_sequence);
    assert_eq!(receipt.completion_journal_sequence, result.journal_entry.journal_sequence);
    assert_eq!(
        receipt.receipt_ref,
        format!(
            "postcondition:{}:{}",
            action.transport_action_id, result.journal_entry.journal_sequence
        )
    );
    assert!(matches!(
        &result.journal_entry.transition,
        ConsequentialJournalTransition::PostconditionReceiptRecorded { receipt: durable }
            if durable == receipt
    ));

    drop(journal);
    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .latest_action_postcondition_receipt(action.transport_action_id)
            .await,
        Some(receipt.clone())
    );
    assert_eq!(
        reopened.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unresolved_unknowns_are_durable_and_never_become_verified_success() {
    let path = journal_path("action-postcondition-receipt-unknown");
    let action = envelope();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);
    linearize(&journal, &action).await;

    let input = bind_and_observe(
        &bridge,
        &journal,
        &action,
        "reconcile:postcondition-receipt:unknown",
        vec![
            evidence(
                "post:visible",
                ConsequentialPostconditionStatus::VerifiedPass,
                "evidence:visible:pass",
            ),
            evidence(
                "post:enabled",
                ConsequentialPostconditionStatus::Unknown,
                "evidence:enabled:unknown",
            ),
        ],
    )
    .await;
    let result = reconcile_consequential_postconditions(&bridge, &journal, input)
        .await
        .unwrap();

    assert_eq!(
        result.postcondition_receipt.verdict,
        ActionPostconditionVerdict::ReconciliationRequired
    );
    assert_eq!(
        result.postcondition_receipt.unresolved_unknown_contract_refs,
        vec!["post:enabled"]
    );
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert!(journal
        .record_committed(action.transport_action_id)
        .await
        .is_err());

    drop(journal);
    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    assert_eq!(
        reopened
            .latest_action_postcondition_receipt(action.transport_action_id)
            .await
            .unwrap()
            .unresolved_unknown_contract_refs,
        vec!["post:enabled"]
    );

    let _ = std::fs::remove_file(path);
}
