use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    ConsequentialJournal, ConsequentialJournalTransition, DispatchLinearizationReceipt,
    DispatchPreparationReceipt, LiveBridge, ProviderObservationBinding,
};
use localview_protocol::{
    DispatchResult, EventContinuityState, PrincipalRef, ProviderElementRealization,
    ProviderElementRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef, TransportResult,
};
use localview_windows_observe_runtime::{
    validate_uia_dispatch_authority, WindowsUiaActionPreflightReceipt,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaDispatchAuthorityError, WindowsUiaDispatchRevalidationReceipt,
};
use localview_windows_uia_provider::{
    WindowsUiaElementLeaseReceipt, WindowsUiaPattern,
};
use thiserror::Error;
use uuid::Uuid;

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-windows-{label}-{}.jsonl", Uuid::new_v4()))
}

fn session() -> SessionId {
    Uuid::from_u128(0x7101)
}

fn provider() -> ProviderIncarnationRef {
    ProviderIncarnationRef::from("provider:windows-uia:authority-fence")
}

fn target() -> TargetIncarnationRef {
    TargetIncarnationRef::from("target:windows:authority-fence")
}

fn authority(cut: &str) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:authority-fence"),
        acting_principal_ref: PrincipalRef::from("principal:acting:authority-fence"),
        authorization_revision: "authorization:authority-fence:v7".into(),
        precondition_snapshot_cut_ref: cut.into(),
        provider_incarnation_ref: provider(),
        target_incarnation_ref: target(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:focus-current".into()],
    }
}

fn element_ref(cut: &str) -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: provider(),
        target_incarnation_ref: target(),
        opaque_provider_element_id: "uia-runtime:[71,1]".into(),
        semantic_locator_hints: vec!["automation_id=authority-fence".into()],
        parent_surface_ref: Some("window:authority-fence".into()),
        acquisition_cut_ref: cut.into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

fn dispatch_revalidation(
    metadata: ActionEnvelopeMetadata,
    cut: &str,
) -> WindowsUiaDispatchRevalidationReceipt {
    let element_ref = element_ref(cut);
    WindowsUiaDispatchRevalidationReceipt {
        authority: metadata.clone(),
        preflight: WindowsUiaActionPreflightReceipt {
            authority: metadata,
            snapshot_cut_ref: cut.into(),
            cache_revision_ref: "cache:authority-fence:1".into(),
            observed_digest: "digest:authority-fence:1".into(),
            element_ref: element_ref.clone(),
            required_pattern: WindowsUiaPattern::Toggle,
        },
        element_lease: WindowsUiaElementLeaseReceipt {
            snapshot_cut_ref: cut.into(),
            provider_incarnation_ref: provider(),
            target_incarnation_ref: target(),
            element_ref,
        },
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake authorization revalidator failure")]
struct FakeAuthorizationError;

#[derive(Debug, Clone)]
struct FakeAuthorizationRevalidator {
    receipt: WindowsUiaAuthorizationRevalidationReceipt,
}

impl WindowsUiaAuthorizationRevalidator for FakeAuthorizationRevalidator {
    type Error = FakeAuthorizationError;

    fn revalidate(
        &self,
        _action_id: Uuid,
        _authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
        Ok(self.receipt.clone())
    }
}

fn revalidator_for(action_id: Uuid, metadata: &ActionEnvelopeMetadata) -> FakeAuthorizationRevalidator {
    FakeAuthorizationRevalidator {
        receipt: WindowsUiaAuthorizationRevalidationReceipt {
            action_id,
            decision_principal_ref: metadata.decision_principal_ref.clone(),
            acting_principal_ref: metadata.acting_principal_ref.clone(),
            authorization_revision: metadata.authorization_revision.clone(),
        },
    }
}

async fn bind_bridge(bridge: &LiveBridge) {
    bridge
        .bind_provider_observation(ProviderObservationBinding {
            session_id: session(),
            generation: 1,
            provider_incarnation_ref: provider(),
            target_incarnation_ref: target(),
            initial_continuity: EventContinuityState::OrderingOpaque,
            sequence_baseline: Some(0),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn exact_canonical_authority_is_revalidated_and_durably_recorded_before_dispatch() {
    let bridge = LiveBridge::new(64, 8);
    bind_bridge(&bridge).await;
    let metadata = authority("cut:authority-fence:1");
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Focus, metadata.clone())
        .await
        .unwrap();

    let path = journal_path("authority-success");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal
        .record_authorization(
            queued.action.id,
            metadata.authorization_revision.clone(),
            false,
        )
        .await
        .unwrap();

    let revalidation = dispatch_revalidation(metadata.clone(), "cut:authority-fence:1");
    let receipt = validate_uia_dispatch_authority(
        &bridge,
        &journal,
        session(),
        queued.action.id,
        revalidation.clone(),
        &revalidator_for(queued.action.id, &metadata),
    )
    .await
    .unwrap();

    assert_eq!(receipt.action_id, queued.action.id);
    assert_eq!(receipt.authority, metadata);
    assert_eq!(receipt.dispatch_revalidation, revalidation);
    assert_eq!(receipt.authorization_journal_sequence, 3);

    let entries = journal.entries_for(queued.action.id).await;
    assert_eq!(entries.len(), 3);
    assert!(matches!(
        &entries[2].transition,
        ConsequentialJournalTransition::AuthorizationRecorded {
            authorization_revision,
            revalidated: true,
        } if authorization_revision == "authorization:authority-fence:v7"
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn principal_substitution_fails_before_journal_revalidation_is_appended() {
    let bridge = LiveBridge::new(64, 8);
    bind_bridge(&bridge).await;
    let metadata = authority("cut:authority-fence:2");
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Focus, metadata.clone())
        .await
        .unwrap();

    let path = journal_path("authority-principal-substitution");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();

    let mut forged = revalidator_for(queued.action.id, &metadata);
    forged.receipt.acting_principal_ref = PrincipalRef::from("principal:acting:forged");

    let error = validate_uia_dispatch_authority(
        &bridge,
        &journal,
        session(),
        queued.action.id,
        dispatch_revalidation(metadata, "cut:authority-fence:2"),
        &forged,
    )
    .await
    .unwrap_err();

    assert_eq!(error, WindowsUiaDispatchAuthorityError::AuthorizationReceiptMismatch);
    assert_eq!(journal.entries_for(queued.action.id).await.len(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn previously_linearized_action_cannot_be_reauthorized_for_blind_redispatch() {
    let bridge = LiveBridge::new(64, 8);
    bind_bridge(&bridge).await;
    let metadata = authority("cut:authority-fence:3");
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Focus, metadata.clone())
        .await
        .unwrap();

    let path = journal_path("authority-linearized");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    let authorized = journal
        .record_authorization(
            queued.action.id,
            metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    journal
        .record_dispatch_prepared(
            queued.action.id,
            DispatchPreparationReceipt {
                receipt_ref: "dispatch-prepared:authority-fence:3".into(),
                authorization_journal_sequence: authorized.journal_sequence,
                precondition_snapshot_cut_ref: metadata.precondition_snapshot_cut_ref.clone(),
                provider_incarnation_ref: metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    journal
        .record_dispatch_linearized(
            queued.action.id,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:authority-fence:3".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();

    let error = validate_uia_dispatch_authority(
        &bridge,
        &journal,
        session(),
        queued.action.id,
        dispatch_revalidation(metadata.clone(), "cut:authority-fence:3"),
        &revalidator_for(queued.action.id, &metadata),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        WindowsUiaDispatchAuthorityError::JournalStateNotDispatchable { .. }
    ));
    assert_eq!(journal.entries_for(queued.action.id).await.len(), 4);

    let _ = std::fs::remove_file(path);
}
