use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialRecoveryState, DispatchPreparationReceipt,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    PrincipalRef, ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaAttachedRecoveryDisposition,
    plan_attached_consequential_recovery,
};
use localview_windows_uia_provider::WindowsUiaEventDrain;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake provider error")]
struct FakeProviderError;

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    snapshot_calls: Arc<Mutex<usize>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:recovery-plan"),
            target: TargetIncarnationRef::from("target:windows:recovery-plan"),
            snapshot_calls: Arc::new(Mutex::new(0)),
        }
    }

    fn snapshot_calls(&self) -> usize {
        *self.snapshot_calls.lock().unwrap()
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[recovery-plan]".into(),
                semantic_locator_hints: vec![],
                parent_surface_ref: Some("window:recovery-plan".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("window".into()),
            name: Some("Recovery Plan".into()),
            control_type: Some("uia_control_type:50032".into()),
            automation_id: None,
            class_name: Some("Window".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::new(),
        };
        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:recovery-plan".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![node],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 1,
                    properties_read: 1,
                    max_depth_observed: 0,
                    exhausted: vec![],
                    incomplete: false,
                },
                completeness: ReconciliationCompleteness::Established,
                incompleteness_debt: vec![],
            })
            .unwrap()
    }
}

impl WindowsObserveProvider for FakeProvider {
    type Attachment = FakeAttachment;
    type Subscription = FakeSubscription;
    type Error = FakeProviderError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.provider.clone()
    }

    fn attach(&self, _selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error> {
        Ok(FakeAttachment(self.target.clone()))
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.0.clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        _capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        Ok(FakeSubscription(WindowsObserveSubscriptionLineage {
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: attachment.0.clone(),
            sequence_baseline: 0,
        }))
    }

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage {
        subscription.0.clone()
    }

    fn drain_events(
        &self,
        _subscription: &Self::Subscription,
        _limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error> {
        Ok(WindowsUiaEventDrain {
            events: vec![],
            dropped_before_drain: 0,
            latest_sequence: 0,
        })
    }

    fn snapshot(
        &self,
        _attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        _surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        *self.snapshot_calls.lock().unwrap() += 1;
        Ok(self.build_snapshot(snapshot_cut_ref))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x7901)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x7902,
        expected_process_id: 79,
        selection_nonce: Uuid::from_u128(0x7903),
    }
}

fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-v43-attachment-recovery-plan-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn envelope(
    label: &str,
    session_id: SessionId,
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id,
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from(format!("principal:planner:{label}")),
            acting_principal_ref: PrincipalRef::from(format!("principal:executor:{label}")),
            authorization_revision: format!("authorization:{label}:v1"),
            precondition_snapshot_cut_ref: format!("cut:{label}:before"),
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![format!("postcondition:{label}")],
        },
    }
}

#[tokio::test]
async fn recovery_plan_is_exact_attachment_bound_ordered_and_side_effect_free() {
    let provider = FakeProvider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        localview_live_bridge::LiveBridge::new(64, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime.attach(session(), selection()).await.unwrap();
    assert_eq!(provider.snapshot_calls(), 1);

    let path = path();
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    let not_dispatched = envelope(
        "not-dispatched",
        session(),
        provider.provider.clone(),
        provider.target.clone(),
    );
    journal
        .record_intent_admitted(not_dispatched.clone())
        .await
        .unwrap();
    journal
        .record_authorization(
            not_dispatched.transport_action_id,
            not_dispatched.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();

    let prepared = envelope(
        "prepared",
        session(),
        provider.provider.clone(),
        provider.target.clone(),
    );
    journal.record_intent_admitted(prepared.clone()).await.unwrap();
    let authorization = journal
        .record_authorization(
            prepared.transport_action_id,
            prepared.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let admission = journal
        .record_dispatch_prepared(
            prepared.transport_action_id,
            DispatchPreparationReceipt {
                receipt_ref: "prepared:recovery-plan".into(),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: prepared
                    .metadata
                    .precondition_snapshot_cut_ref
                    .clone(),
                provider_incarnation_ref: prepared.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: prepared.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    drop(admission);

    let wrong_target = envelope(
        "wrong-target",
        session(),
        provider.provider.clone(),
        TargetIncarnationRef::from("target:windows:different"),
    );
    journal
        .record_intent_admitted(wrong_target.clone())
        .await
        .unwrap();

    let inventory_before = journal.recovery_inventory().await;
    let plan = plan_attached_consequential_recovery(&journal, &runtime, session())
        .await
        .unwrap();

    assert_eq!(plan.session_id, session());
    assert_eq!(plan.provider_incarnation_ref, provider.provider);
    assert_eq!(plan.target_incarnation_ref, provider.target);
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].action_id, not_dispatched.transport_action_id);
    assert_eq!(
        plan.entries[0].recovery_state,
        ConsequentialRecoveryState::AuthorizedNotDispatched
    );
    assert_eq!(
        plan.entries[0].disposition,
        WindowsUiaAttachedRecoveryDisposition::NotDispatched
    );
    assert_eq!(plan.entries[1].action_id, prepared.transport_action_id);
    assert_eq!(
        plan.entries[1].recovery_state,
        ConsequentialRecoveryState::DispatchPrepared
    );
    assert_eq!(
        plan.entries[1].disposition,
        WindowsUiaAttachedRecoveryDisposition::VerificationRequired
    );
    assert!(
        plan.entries
            .windows(2)
            .all(|pair| pair[0].latest_journal_sequence < pair[1].latest_journal_sequence)
    );
    assert!(
        plan.entries
            .iter()
            .all(|entry| entry.action_id != wrong_target.transport_action_id),
        "journal debt from a different target incarnation must not be attached to this runtime"
    );
    assert_eq!(journal.recovery_inventory().await, inventory_before);
    assert_eq!(
        provider.snapshot_calls(),
        1,
        "planning may inspect the immutable attached snapshot but must not mint a new provider observation"
    );

    let _ = std::fs::remove_file(path);
}
