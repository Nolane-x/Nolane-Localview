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
        "localview-v43-attachment-recovery-debt-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn envelope(
    label: &str,
    session_id: SessionId,
    provider: &str,
    target: &str,
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
async fn attachment_recovery_debt_is_exact_lineage_and_latest_sequence_ordered() {
    let path = journal_path();
    let session = SessionId::new_v4();
    let other_session = SessionId::new_v4();
    let provider = ProviderIncarnationRef::from("provider:attached:1");
    let target = TargetIncarnationRef::from("target:attached:1");

    let exact_first = envelope(
        "exact-first",
        session,
        provider.as_ref(),
        target.as_ref(),
    );
    let wrong_provider = envelope(
        "wrong-provider",
        session,
        "provider:other:1",
        target.as_ref(),
    );
    let wrong_target = envelope(
        "wrong-target",
        session,
        provider.as_ref(),
        "target:other:1",
    );
    let wrong_session = envelope(
        "wrong-session",
        other_session,
        provider.as_ref(),
        target.as_ref(),
    );
    let exact_second = envelope(
        "exact-second",
        session,
        provider.as_ref(),
        target.as_ref(),
    );

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let first_admitted = journal
        .record_intent_admitted(exact_first.clone())
        .await
        .unwrap();
    journal
        .record_intent_admitted(wrong_provider)
        .await
        .unwrap();
    journal.record_intent_admitted(wrong_target).await.unwrap();
    journal
        .record_intent_admitted(wrong_session)
        .await
        .unwrap();
    journal
        .record_intent_admitted(exact_second.clone())
        .await
        .unwrap();
    let second_authorized = journal
        .record_authorization(
            exact_second.transport_action_id,
            exact_second.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    drop(journal);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let debt = reopened
        .recovery_debt_for_attachment(session, &provider, &target)
        .await;

    assert_eq!(debt.len(), 2);
    assert_eq!(debt[0].action_id, exact_first.transport_action_id);
    assert_eq!(debt[0].session_id, session);
    assert_eq!(debt[0].provider_incarnation_ref, provider);
    assert_eq!(debt[0].target_incarnation_ref, target);
    assert_eq!(
        debt[0].expected_postcondition_contract_refs,
        exact_first.metadata.expected_postcondition_contract_refs
    );
    assert_eq!(debt[0].recovery_state, ConsequentialRecoveryState::Admitted);
    assert_eq!(
        debt[0].latest_journal_sequence,
        first_admitted.journal_sequence
    );

    assert_eq!(debt[1].action_id, exact_second.transport_action_id);
    assert_eq!(
        debt[1].recovery_state,
        ConsequentialRecoveryState::AuthorizedNotDispatched
    );
    assert_eq!(
        debt[1].latest_journal_sequence,
        second_authorized.journal_sequence
    );
    assert!(
        debt.windows(2)
            .all(|pair| pair[0].latest_journal_sequence < pair[1].latest_journal_sequence)
    );

    let _ = std::fs::remove_file(path);
}
