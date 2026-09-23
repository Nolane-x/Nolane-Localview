use std::path::PathBuf;

use localview_live_bridge::{
    verify_managed_web_payload_binding, ActionEnvelopeMetadata, ActionIdempotencyClass,
    ActionRiskClass, BridgeActionKind, ConsequentialJournal, LiveBridge,
    ManagedWebPayloadCommitmentKey, ManagedWebPayloadRef, ProviderObservationBinding,
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
        decision_principal_ref: PrincipalRef::from("principal:managed-web-payload:decision"),
        acting_principal_ref: PrincipalRef::from("principal:managed-web-payload:acting"),
        authorization_revision: "authorization:managed-web-payload:v1".into(),
        precondition_snapshot_cut_ref: "cut:managed-web-payload:1".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        risk_class: ActionRiskClass::Unknown,
        idempotency_class: ActionIdempotencyClass::Unknown,
        expected_postcondition_contract_refs: vec!["postcondition:managed-web-payload".into()],
    }
}

async fn direct_action(
    kind: BridgeActionKind,
) -> localview_live_bridge::CanonicalQueuedAction {
    let bridge = LiveBridge::new(32, 8);
    let session_id: SessionId = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:managed-web:payload-test");
    let target = TargetIncarnationRef::from("target:managed-web:payload-test");
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

    bridge
        .bind_direct_canonical_action(
            session_id,
            Some("@eabc123".into()),
            kind,
            metadata(provider, target),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn managed_web_payload_binding_is_opaque_immutable_and_process_bound() {
    let queued = direct_action(BridgeActionKind::TypeText {
        text: String::new(),
        clear_first: true,
    })
    .await;
    let path = journal_path("managed-web-payload-binding");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let admitted = journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal.record_intent_operation_bound(&queued).await.unwrap();

    let key = ManagedWebPayloadCommitmentKey::generate().unwrap();
    let payload_ref = ManagedWebPayloadRef(Uuid::new_v4());
    let payload = br#"{"clear_first":true,"text":"private fixture value"}"#;
    let binding = journal
        .record_managed_web_payload_binding(&queued, &key, payload_ref, payload)
        .await
        .unwrap();

    assert_eq!(binding.action_id, queued.action.id);
    assert_eq!(binding.intent_journal_sequence, admitted.journal_sequence);
    assert_eq!(binding.payload_ref, payload_ref);
    assert_eq!(
        binding.operation,
        localview_live_bridge::CanonicalActionOperation::InputText
    );
    assert_eq!(binding.payload_len, payload.len() as u64);
    assert_eq!(
        binding.commitment_algorithm,
        "hmac-sha256-process-v1"
    );

    let encoded = serde_json::to_vec(&binding).unwrap();
    assert!(
        !encoded
            .windows(b"private fixture value".len())
            .any(|window| window == b"private fixture value"),
        "durable binding must not serialize payload plaintext"
    );
    assert!(verify_managed_web_payload_binding(&key, &binding, payload).is_ok());
    assert!(
        verify_managed_web_payload_binding(
            &key,
            &binding,
            br#"{"clear_first":true,"text":"tampered"}"#,
        )
        .is_err()
    );

    assert!(
        journal
            .record_managed_web_payload_binding(&queued, &key, payload_ref, payload)
            .await
            .is_err(),
        "one admitted action must not accept a second payload companion"
    );

    drop(journal);
    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let reopened_binding = reopened
        .managed_web_payload_binding(queued.action.id)
        .await
        .unwrap()
        .expect("durable managed-WebView payload metadata");
    assert_eq!(reopened_binding, binding);
    assert!(verify_managed_web_payload_binding(&key, &reopened_binding, payload).is_ok());

    let replacement_key = ManagedWebPayloadCommitmentKey::generate().unwrap();
    assert!(
        verify_managed_web_payload_binding(&replacement_key, &reopened_binding, payload).is_err(),
        "restart key must not restore payload execution authority"
    );

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn managed_web_payload_binding_rejects_payload_free_operations() {
    let queued = direct_action(BridgeActionKind::Click).await;
    let path = journal_path("managed-web-payload-reject-click");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal.record_intent_operation_bound(&queued).await.unwrap();

    let key = ManagedWebPayloadCommitmentKey::generate().unwrap();
    assert!(
        journal
            .record_managed_web_payload_binding(
                &queued,
                &key,
                ManagedWebPayloadRef(Uuid::new_v4()),
                b"must-not-bind",
            )
            .await
            .is_err()
    );

    let _ = std::fs::remove_file(&path);
}
