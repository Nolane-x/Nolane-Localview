use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, ConsequentialJournal, ConsequentialRecoveryDebtDisposition,
    DispatchPreparationReceipt, LiveBridge,
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
    WindowsObserveSubscriptionLineage, plan_attached_consequential_recovery,
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
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:set-value-recovery"),
            target: TargetIncarnationRef::from("target:windows:set-value-recovery"),
        }
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[set-value-recovery]".into(),
                semantic_locator_hints: vec!["automation_id=value-input".into()],
                parent_surface_ref: Some("window:set-value-recovery".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("textbox".into()),
            name: Some("Value".into()),
            control_type: Some("uia_control_type:50004".into()),
            automation_id: Some("value-input".into()),
            class_name: Some("Edit".into()),
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
                surface_scope: "window:set-value-recovery".into(),
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

    fn attach(
        &self,
        _selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error> {
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
        Ok(self.build_snapshot(snapshot_cut_ref))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn prepared_set_value_without_live_payload_is_reconciliation_only_after_restart() {
    let session_id: SessionId = Uuid::from_u128(0x8b01);
    let provider = FakeProvider::new();
    let bridge = LiveBridge::new(64, 8);
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime
        .attach(
            session_id,
            UserSelectedWindowTarget {
                native_window_handle: 0x8b02,
                expected_process_id: 83,
                selection_nonce: Uuid::from_u128(0x8b03),
            },
        )
        .await
        .unwrap();

    let snapshot = runtime.current_semantic_snapshot(session_id).await.unwrap();
    let element_ref = snapshot.nodes()[0].element_ref.clone();
    let payload_ref = Uuid::from_u128(0x8b04);
    let payload_contract = format!(
        "lvpc:payload-equality:v1:{{\"mode\":\"replace_value\",\"payload_ref\":\"{payload_ref}\"}}"
    );
    let queued = bridge
        .bind_direct_canonical_action(
            session_id,
            Some(element_ref.opaque_provider_element_id.clone()),
            BridgeActionKind::Click,
            ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from("principal:set-value-recovery:decision"),
                acting_principal_ref: PrincipalRef::from("principal:set-value-recovery:acting"),
                authorization_revision: "authorization:set-value-recovery:v1".into(),
                precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().to_owned(),
                provider_incarnation_ref: provider.provider.clone(),
                target_incarnation_ref: provider.target.clone(),
                risk_class: ActionRiskClass::DestructiveOrIrreversible,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![payload_contract],
            },
        )
        .await
        .unwrap();

    let path = PathBuf::from(format!(
        "{}{}",
        std::env::temp_dir()
            .join(format!("localview-set-value-recovery-{}", Uuid::new_v4()))
            .display(),
        ".jsonl"
    ));
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal
        .record_intent_operation_bound_explicit(&queued, CanonicalActionOperation::SetValue)
        .await
        .unwrap();
    let authorization = journal
        .record_authorization(
            queued.action.id,
            queued.envelope.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let prepared = journal
        .record_dispatch_prepared(
            queued.action.id,
            DispatchPreparationReceipt {
                receipt_ref: "prepared:set-value-recovery".into(),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: queued
                    .envelope
                    .metadata
                    .precondition_snapshot_cut_ref
                    .clone(),
                provider_incarnation_ref: provider.provider.clone(),
                target_incarnation_ref: provider.target.clone(),
            },
        )
        .await
        .unwrap();
    drop(prepared);

    let reopened = ConsequentialJournal::open(&path).await.unwrap();
    let plan = plan_attached_consequential_recovery(&reopened, &runtime, session_id)
        .await
        .unwrap();
    assert_eq!(plan.entries.len(), 1);
    assert_eq!(plan.entries[0].action_id, queued.action.id);
    assert_eq!(
        plan.entries[0].disposition,
        ConsequentialRecoveryDebtDisposition::ReconciliationRequired,
        "restart has no process-local SetValue payload/key, so PREPARED SetValue debt must never enter generic observation recovery"
    );

    let _ = std::fs::remove_file(path);
}
