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
        "localview-v43-recovery-inventory-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn envelope(label: &str) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: SessionId::new_v4(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from(format!("principal:planner:{label}")),
            acting_principal_ref: PrincipalRef::from(format!("principal:executor:{label}")),
            authorization_revision: format!("auth:{label}:v1"),
            precondition_snapshot_cut_ref: format!("cut:{label}:before"),
            provider_incarnation_ref: ProviderIncarnationRef::from(format!(
                "provider:{label}:1"
            )),
            target_incarnation_ref: TargetIncarnationRef::from(format!("target:{label}:1")),
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![format!("postcondition:{label}:visible")],
        },
    }
}

#[tokio::test]
async fn reopened_journal_inventory_is_one_entry_per_action_in_latest_sequence_order() {
    let path = journal_path();
    let first = envelope("first");
    let second = envelope("second");

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let first_admitted = journal.record_intent_admitted(first.clone()).await.unwrap();
    let second_admitted = journal.record_intent_admitted(second.clone()).await.unwrap();
    let first_authorized = journal
        .record_authorization(
            first.transport_action_id,
            first.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();

    assert!(first_admitted.journal_sequence < second_admitted.journal_sequence);
    assert!(second_admitted.journal_sequence < first_authorized.journal_sequence);
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let inventory = reopened.recovery_inventory().await;

    assert_eq!(inventory.len(), 2);
    assert_eq!(inventory[0].action_id, second.transport_action_id);
    assert_eq!(inventory[0].recovery_state, ConsequentialRecoveryState::Admitted);
    assert_eq!(
        inventory[0].latest_journal_sequence,
        second_admitted.journal_sequence
    );
    assert_eq!(inventory[1].action_id, first.transport_action_id);
    assert_eq!(
        inventory[1].recovery_state,
        ConsequentialRecoveryState::AuthorizedNotDispatched
    );
    assert_eq!(
        inventory[1].latest_journal_sequence,
        first_authorized.journal_sequence
    );
    assert!(
        inventory
            .windows(2)
            .all(|pair| pair[0].latest_journal_sequence < pair[1].latest_journal_sequence)
    );

    let _ = std::fs::remove_file(path);
}
