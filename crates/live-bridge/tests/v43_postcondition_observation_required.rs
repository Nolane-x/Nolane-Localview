use std::path::PathBuf;

use localview_live_bridge::{
    reconcile_consequential_postconditions, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, CanonicalActionEnvelope, ConsequentialJournal,
    ConsequentialPostconditionEvidence, ConsequentialPostconditionReconciliationReceipt,
    ConsequentialPostconditionStatus, DispatchLinearizationReceipt, DispatchPreparationReceipt,
    LiveBridge, ProviderObservationBinding,
};
use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderIncarnationRef,
    ReconciliationCompleteness, ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
use uuid::Uuid;

fn path() -> PathBuf {
    std::env::temp_dir().join(format!("localview-post-observation-required-{}.jsonl", Uuid::new_v4()))
}

#[tokio::test]
async fn reconciliation_consumes_only_a_journal_minted_post_dispatch_observation_receipt() {
    let path = path();
    let action = CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner:causal-postcondition"),
            acting_principal_ref: PrincipalRef::from("principal:executor:causal-postcondition"),
            authorization_revision: "auth:causal-postcondition:v1".into(),
            precondition_snapshot_cut_ref: "cut:before-dispatch".into(),
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:uia:causal-postcondition:1"),
            target_incarnation_ref: TargetIncarnationRef::from("target:window:causal-postcondition:1"),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["post:visible".into()],
        },
    };
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let bridge = LiveBridge::new(32, 8);

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
                receipt_ref: "prepared:causal-postcondition".into(),
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
                receipt_ref: "dispatch:causal-postcondition".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();

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

    let observation = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let cut = observation.snapshot_cut_ref().to_owned();
    let provider_snapshot = ReconciliationSnapshotReceipt {
        receipt_id: "reconcile:causal-postcondition".into(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref: cut.clone(),
        surface_scope: "selected-window".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:v1".into(),
        permission_visibility_revision: "visibility:v1".into(),
        capture_sequence: 3,
        observed_digest: "digest:causal-postcondition".into(),
        incompleteness_debt: Vec::new(),
    };
    assert!(bridge
        .record_reconciliation(action.session_id, provider_snapshot.clone())
        .await);
    let observation_receipt = journal
        .complete_postcondition_observation(observation, provider_snapshot)
        .await
        .unwrap();

    let typed_receipt = ConsequentialPostconditionReconciliationReceipt::from_observation(
        observation_receipt,
        vec![ConsequentialPostconditionEvidence {
            contract_ref: "post:visible".into(),
            status: ConsequentialPostconditionStatus::VerifiedPass,
            receipt_ref: "postcondition:visible:pass".into(),
        }],
    );
    let result = reconcile_consequential_postconditions(&bridge, &journal, typed_receipt)
        .await
        .unwrap();
    assert_eq!(result.world_outcome, WorldOutcome::VerifiedExpected);
    assert!(result.postconditions_verified);

    let _ = std::fs::remove_file(path);
}
