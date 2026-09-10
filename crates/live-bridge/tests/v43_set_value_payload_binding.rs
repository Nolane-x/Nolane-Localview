use std::path::PathBuf;

use localview_live_bridge::{
    verify_set_value_payload_binding, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, BridgeActionKind, CanonicalActionOperation, ConsequentialJournal, LiveBridge,
    ProviderObservationBinding, SetValueCommitmentKey, SetValueMode, SetValuePayloadRef,
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
        decision_principal_ref: PrincipalRef::from("principal:set-value-binding:decision"),
        acting_principal_ref: PrincipalRef::from("principal:set-value-binding:acting"),
        authorization_revision: "authorization:set-value-binding:v1".into(),
        precondition_snapshot_cut_ref: "cut:set-value-binding:1".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        risk_class: ActionRiskClass::DestructiveOrIrreversible,
        idempotency_class: ActionIdempotencyClass::Irreversible,
        expected_postcondition_contract_refs: vec!["postcondition:set-value-binding".into()],
    }
}

async fn admitted_set_value_action() -> (LiveBridge, localview_live_bridge::CanonicalQueuedAction) {
    let bridge = LiveBridge::new(32, 8);
    let session_id: SessionId = Uuid::from_u128(0x9a01);
    let provider = ProviderIncarnationRef::from("provider:windows-uia:set-value-binding");
    let target = TargetIncarnationRef::from("target:windows:set-value-binding");
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

    let queued = bridge
        .bind_direct_canonical_action(
            session_id,
            None,
            BridgeActionKind::TypeText {
                text: String::new(),
                clear_first: false,
            },
            metadata(provider, target),
        )
        .await
        .unwrap();
    (bridge, queued)
}

#[tokio::test]
async fn set_value_payload_binding_is_opaque_immutable_and_exact() {
    let (_bridge, queued) = admitted_set_value_action().await;
    let path = journal_path("set-value-payload-binding");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let admitted = journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal
        .record_intent_operation_bound_explicit(&queued, CanonicalActionOperation::SetValue)
        .await
        .unwrap();

    let key = SetValueCommitmentKey::generate().expect("OS randomness should generate process key");
    let payload_ref = SetValuePayloadRef(Uuid::new_v4());
    let payload = b"fixture-value";
    let binding = journal
        .record_set_value_payload_binding(
            &queued,
            &key,
            payload_ref,
            SetValueMode::ReplaceValue,
            payload,
        )
        .await
        .unwrap();

    assert_eq!(binding.action_id, queued.action.id);
    assert_eq!(binding.intent_journal_sequence, admitted.journal_sequence);
    assert_eq!(binding.payload_ref, payload_ref);
    assert_eq!(binding.mode, SetValueMode::ReplaceValue);
    assert_eq!(binding.payload_utf8_len, 13);
    assert_eq!(binding.commitment_algorithm, "hmac-sha256-process-v1");

    let encoded = serde_json::to_vec(&binding).unwrap();
    assert!(
        !encoded.windows(payload.len()).any(|window| window == payload),
        "durable payload metadata must never serialize caller plaintext"
    );
    assert!(verify_set_value_payload_binding(&key, &binding, payload).is_ok());
    assert!(verify_set_value_payload_binding(&key, &binding, b"different").is_err());

    assert!(
        journal
            .record_set_value_payload_binding(
                &queued,
                &key,
                payload_ref,
                SetValueMode::ReplaceValue,
                payload,
            )
            .await
            .is_err(),
        "payload binding must be immutable/create-new for one admitted action"
    );

    drop(journal);
    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let reopened_binding = reopened
        .set_value_payload_binding(queued.action.id)
        .await
        .unwrap()
        .expect("opaque payload metadata must survive journal reopen");
    assert_eq!(reopened_binding, binding);
    assert!(verify_set_value_payload_binding(&key, &reopened_binding, payload).is_ok());

    let fresh_process_key =
        SetValueCommitmentKey::generate().expect("OS randomness should generate replacement key");
    assert!(
        verify_set_value_payload_binding(&fresh_process_key, &reopened_binding, payload).is_err(),
        "a restarted process key must not validate old payload authority"
    );

    let _ = std::fs::remove_file(&path);
}
