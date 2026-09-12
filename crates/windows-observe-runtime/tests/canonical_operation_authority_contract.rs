use std::path::PathBuf;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, ConsequentialJournal, LiveBridge, ProviderObservationBinding,
};
use localview_protocol::{
    EventContinuityState, PrincipalRef, ProviderElementRealization, ProviderElementRef,
    ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    validate_uia_dispatch_authority, WindowsUiaActionPreflightReceipt,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaDispatchAuthorityError, WindowsUiaDispatchRevalidationReceipt,
};
use localview_windows_uia_provider::{WindowsUiaElementLeaseReceipt, WindowsUiaPattern};
use thiserror::Error;
use uuid::Uuid;

fn path() -> PathBuf {
    std::env::temp_dir().join(format!("localview-native-operation-{}.jsonl", Uuid::new_v4()))
}

fn session() -> SessionId {
    Uuid::from_u128(0x8911)
}

fn provider() -> ProviderIncarnationRef {
    ProviderIncarnationRef::from("provider:windows-uia:native-operation")
}

fn target() -> TargetIncarnationRef {
    TargetIncarnationRef::from("target:windows:native-operation")
}

fn metadata() -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:native-operation:decision"),
        acting_principal_ref: PrincipalRef::from("principal:native-operation:acting"),
        authorization_revision: "authorization:native-operation:v1".into(),
        precondition_snapshot_cut_ref: "cut:native-operation:1".into(),
        provider_incarnation_ref: provider(),
        target_incarnation_ref: target(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:native-operation".into()],
    }
}

fn element_ref() -> ProviderElementRef {
    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref: provider(),
        target_incarnation_ref: target(),
        opaque_provider_element_id: "uia-runtime:[89,1]".into(),
        semantic_locator_hints: vec!["automation_id=native-operation".into()],
        parent_surface_ref: Some("window:native-operation".into()),
        acquisition_cut_ref: "cut:native-operation:1".into(),
        realization: ProviderElementRealization::RealizedCurrent,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

fn revalidation(
    authority: ActionEnvelopeMetadata,
    pattern: WindowsUiaPattern,
) -> WindowsUiaDispatchRevalidationReceipt {
    let element = element_ref();
    WindowsUiaDispatchRevalidationReceipt {
        authority: authority.clone(),
        preflight: WindowsUiaActionPreflightReceipt {
            authority,
            snapshot_cut_ref: "cut:native-operation:1".into(),
            cache_revision_ref: "cache:native-operation:1".into(),
            observed_digest: "digest:native-operation:1".into(),
            element_ref: element.clone(),
            required_pattern: pattern,
        },
        element_lease: WindowsUiaElementLeaseReceipt {
            snapshot_cut_ref: "cut:native-operation:1".into(),
            provider_incarnation_ref: provider(),
            target_incarnation_ref: target(),
            element_ref: element,
        },
    }
}

#[derive(Debug, Error)]
#[error("authorization should not be reached on operation mismatch")]
struct AuthorizationError;

struct Revalidator;

impl WindowsUiaAuthorizationRevalidator for Revalidator {
    type Error = AuthorizationError;

    fn revalidate(
        &self,
        action_id: Uuid,
        authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
        Ok(WindowsUiaAuthorizationRevalidationReceipt {
            action_id,
            decision_principal_ref: authority.decision_principal_ref.clone(),
            acting_principal_ref: authority.acting_principal_ref.clone(),
            authorization_revision: authority.authorization_revision.clone(),
        })
    }
}

async fn bind(bridge: &LiveBridge) {
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
async fn focus_intent_cannot_be_substituted_with_invoke_or_other_uia_pattern() {
    let bridge = LiveBridge::new(32, 8);
    bind(&bridge).await;
    let authority = metadata();
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Focus, authority.clone())
        .await
        .unwrap();

    let journal_path = path();
    let journal = ConsequentialJournal::open(&journal_path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal.record_intent_operation_bound(&queued).await.unwrap();

    let error = validate_uia_dispatch_authority(
        &bridge,
        &journal,
        session(),
        queued.action.id,
        revalidation(authority, WindowsUiaPattern::Invoke),
        &Revalidator,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        WindowsUiaDispatchAuthorityError::CanonicalOperationMismatch {
            canonical: CanonicalActionOperation::Focus,
            requested_pattern: WindowsUiaPattern::Invoke,
        }
    );
    assert_eq!(
        journal.entries_for(queued.action.id).await.len(),
        1,
        "operation mismatch must fail before authorization is appended"
    );

    let _ = std::fs::remove_file(&journal_path);
    let _ = std::fs::remove_file(format!(
        "{}.operation-{}.json",
        journal_path.display(),
        queued.action.id
    ));
}

#[tokio::test]
async fn activate_intent_without_durable_operation_binding_fails_closed() {
    let bridge = LiveBridge::new(32, 8);
    bind(&bridge).await;
    let authority = metadata();
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Click, authority.clone())
        .await
        .unwrap();

    let journal_path = path();
    let journal = ConsequentialJournal::open(&journal_path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();

    let error = validate_uia_dispatch_authority(
        &bridge,
        &journal,
        session(),
        queued.action.id,
        revalidation(authority, WindowsUiaPattern::Invoke),
        &Revalidator,
    )
    .await
    .unwrap_err();

    assert_eq!(error, WindowsUiaDispatchAuthorityError::CanonicalOperationMissing);
    assert_eq!(journal.entries_for(queued.action.id).await.len(), 1);

    let _ = std::fs::remove_file(journal_path);
}
