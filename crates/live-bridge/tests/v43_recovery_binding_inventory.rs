use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialRecoveryState,
};
use localview_protocol::{
    PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use uuid::Uuid;

fn journal_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-v43-recovery-binding-inventory-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn envelope(
    session_id: SessionId,
    provider: &str,
    target: &str,
    label: &str,
) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id,
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from(format!("principal:planner:{label}")),
            acting_principal_ref: PrincipalRef::from(format!("principal:executor:{label}")),
            authorization_revision: format!("auth:{label}:v1"),
            precondition_snapshot_cut_ref: format!("cut:{label}:before"),
            provider_incarnation_ref: ProviderIncarnationRef::from(provider),
            target_incarnation_ref: TargetIncarnationRef::from(target),
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![format!("postcondition:{label}:visible")],
        },
    }
}

#[tokio::test]
async fn recovery_binding_inventory_preserves_exact_durable_action_lineage() {
    let path = journal_path();
    let session = SessionId::new_v4();
    let first = envelope(session, "provider:uia:1", "target:window:1", "first");
    let second = envelope(session, "provider:uia:2", "target:window:2", "second");

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let first_entry = journal.record_intent_admitted(first.clone()).await.unwrap();
    let second_entry = journal.record_intent_admitted(second.clone()).await.unwrap();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let bindings = reopened.recovery_binding_inventory().await;

    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].action_id, first.transport_action_id);
    assert_eq!(bindings[0].session_id, session);
    assert_eq!(
        bindings[0].provider_incarnation_ref,
        first.metadata.provider_incarnation_ref
    );
    assert_eq!(
        bindings[0].target_incarnation_ref,
        first.metadata.target_incarnation_ref
    );
    assert_eq!(
        bindings[0].expected_postcondition_contract_refs,
        first.metadata.expected_postcondition_contract_refs
    );
    assert_eq!(bindings[0].recovery_state, ConsequentialRecoveryState::Admitted);
    assert_eq!(bindings[0].latest_journal_sequence, first_entry.journal_sequence);

    assert_eq!(bindings[1].action_id, second.transport_action_id);
    assert_eq!(bindings[1].provider_incarnation_ref, second.metadata.provider_incarnation_ref);
    assert_eq!(bindings[1].target_incarnation_ref, second.metadata.target_incarnation_ref);
    assert_eq!(bindings[1].latest_journal_sequence, second_entry.journal_sequence);

    let exact = reopened
        .recovery_bindings_for_attachment(
            session,
            &ProviderIncarnationRef::from("provider:uia:2"),
            &TargetIncarnationRef::from("target:window:2"),
        )
        .await;
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].action_id, second.transport_action_id);

    let stale_target = reopened
        .recovery_bindings_for_attachment(
            session,
            &ProviderIncarnationRef::from("provider:uia:2"),
            &TargetIncarnationRef::from("target:window:stale"),
        )
        .await;
    assert!(stale_target.is_empty());

    let _ = std::fs::remove_file(path);
}
