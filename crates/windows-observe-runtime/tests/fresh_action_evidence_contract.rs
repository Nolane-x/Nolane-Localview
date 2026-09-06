use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use localview_live_bridge::LiveBridge;
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaEventDrain, WindowsUiaPattern,
    WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Attachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct Subscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fresh action evidence test provider failure")]
struct ProviderError;

#[derive(Debug, Default)]
struct ProviderState {
    snapshots: usize,
}

#[derive(Debug, Clone)]
struct Provider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    state: Arc<Mutex<ProviderState>>,
}

impl Provider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:fresh-action-evidence"),
            target: TargetIncarnationRef::from("target:windows:fresh-action-evidence"),
            state: Arc::new(Mutex::new(ProviderState::default())),
        }
    }

    fn snapshot_count(&self) -> usize {
        self.state.lock().unwrap().snapshots
    }

    fn snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        self.state.lock().unwrap().snapshots += 1;

        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Invoke, WindowsUiaPatternSupport::Supported);
        let mut attributes = BTreeMap::new();
        capabilities.write_attributes(&mut attributes);

        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut.clone(),
                surface_scope: "window:fresh-action-evidence".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: self.snapshot_count() as u64,
                nodes: vec![NativeSemanticNodeObservation {
                    element_ref: ProviderElementRef {
                        provider_family: "windows_uia".into(),
                        provider_incarnation_ref: self.provider.clone(),
                        target_incarnation_ref: self.target.clone(),
                        opaque_provider_element_id: "uia-runtime:[92,1]".into(),
                        semantic_locator_hints: vec!["automation_id=confirm".into()],
                        parent_surface_ref: Some("window:fresh-action-evidence".into()),
                        acquisition_cut_ref: cut,
                        realization: ProviderElementRealization::RealizedCurrent,
                        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
                    },
                    parent_index: None,
                    depth: 0,
                    role: Some("button".into()),
                    name: Some("Confirm".into()),
                    control_type: Some("uia_control_type:50000".into()),
                    automation_id: Some("confirm".into()),
                    class_name: Some("Button".into()),
                    is_enabled: Some(true),
                    is_offscreen: Some(false),
                    attributes,
                }],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 1,
                    properties_read: 14,
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

impl WindowsObserveProvider for Provider {
    type Attachment = Attachment;
    type Subscription = Subscription;
    type Error = ProviderError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.provider.clone()
    }

    fn attach(&self, _selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error> {
        Ok(Attachment(self.target.clone()))
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.0.clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        _capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        Ok(Subscription(WindowsObserveSubscriptionLineage {
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
        Ok(self.snapshot(snapshot_cut_ref))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn refresh_action_evidence_captures_a_new_cut_and_rebinds_the_exact_provider_element() {
    let session_id = SessionId::from_u128(0x9201);
    let provider = Provider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        LiveBridge::new(64, 8),
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
                native_window_handle: 0x9202,
                expected_process_id: 92,
                selection_nonce: Uuid::from_u128(0x9203),
            },
        )
        .await
        .unwrap();

    let before = runtime.current_semantic_snapshot(session_id).await.unwrap();
    let previous_element_ref = before.nodes()[0].element_ref.clone();
    assert_eq!(provider.snapshot_count(), 1);

    let receipt = runtime
        .refresh_uia_action_evidence(session_id, previous_element_ref.clone())
        .await
        .expect("fresh action evidence must capture and bind a new current snapshot");

    assert_eq!(
        provider.snapshot_count(),
        2,
        "refresh must call the provider exactly once"
    );
    assert_eq!(receipt.previous_element_ref, previous_element_ref);
    assert_eq!(
        receipt.refreshed_element_ref.opaque_provider_element_id,
        "uia-runtime:[92,1]"
    );
    assert_ne!(receipt.snapshot_cut_ref, before.snapshot_cut_ref());
    assert_eq!(
        receipt.refreshed_element_ref.acquisition_cut_ref,
        receipt.snapshot_cut_ref
    );

    let current = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_eq!(current.snapshot_cut_ref(), receipt.snapshot_cut_ref);
    assert_eq!(current.observed_digest(), receipt.observed_digest);

    let status = runtime.status(session_id).await.unwrap();
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        status.reconciliation_receipt_id.as_deref(),
        Some(receipt.reconciliation_receipt_ref.as_str())
    );
}
