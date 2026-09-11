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
    WindowsObserveSubscriptionLineage, WindowsObserveVirtualizedItemProvider,
};
use localview_windows_uia_provider::{
    WindowsUiaEventDrain, WindowsUiaItemLookupProperty, WindowsUiaVirtualizedItemQueryReceipt,
    WindowsUiaVirtualizedItemQueryRequest, WindowsUiaVirtualizedItemRealizeReceipt,
    WindowsUiaVirtualizedItemRealizeRequest,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake virtualized-item provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    realized: bool,
    query_calls: usize,
    realize_calls: usize,
    snapshots: usize,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:w03-runtime"),
            target: TargetIncarnationRef::from("target:windows:w03-runtime"),
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (state.query_calls, state.realize_calls, state.snapshots)
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let realized = self.state.lock().unwrap().realized;
        let container_ref = ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            opaque_provider_element_id: "uia-runtime:[503,1]".into(),
            semantic_locator_hints: vec!["automation_id=virtualized-list".into()],
            parent_surface_ref: Some("window:w03-runtime".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        };
        let mut nodes = vec![NativeSemanticNodeObservation {
            element_ref: container_ref,
            parent_index: None,
            depth: 0,
            role: Some("list".into()),
            name: Some("Virtualized items".into()),
            control_type: Some("uia_control_type:50008".into()),
            automation_id: Some("virtualized-list".into()),
            class_name: Some("ListBox".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::new(),
        }];
        if realized {
            nodes.push(NativeSemanticNodeObservation {
                element_ref: ProviderElementRef {
                    provider_family: "windows_uia".into(),
                    provider_incarnation_ref: self.provider.clone(),
                    target_incarnation_ref: self.target.clone(),
                    opaque_provider_element_id: "uia-runtime:[503,255]".into(),
                    semantic_locator_hints: vec!["name=LocalView Virtual Item 255".into()],
                    parent_surface_ref: Some("window:w03-runtime".into()),
                    acquisition_cut_ref: cut.clone(),
                    realization: ProviderElementRealization::RealizedCurrent,
                    lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
                },
                parent_index: Some(0),
                depth: 1,
                role: Some("list item".into()),
                name: Some("LocalView Virtual Item 255".into()),
                control_type: Some("uia_control_type:50007".into()),
                automation_id: None,
                class_name: Some("ListBoxItem".into()),
                is_enabled: Some(true),
                is_offscreen: Some(false),
                attributes: BTreeMap::new(),
            });
        }

        let node_count = nodes.len();
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:w03-runtime".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: self.state.lock().unwrap().snapshots as u64 + 1,
                nodes,
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: node_count,
                    properties_read: node_count * 12,
                    max_depth_observed: usize::from(realized),
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
    type Error = FakeError;

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
        let snapshot = self.build_snapshot(snapshot_cut_ref);
        self.state.lock().unwrap().snapshots += 1;
        Ok(snapshot)
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl WindowsObserveVirtualizedItemProvider for FakeProvider {
    fn query_virtualized_item(
        &self,
        _attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, Self::Error> {
        self.state.lock().unwrap().query_calls += 1;
        let mut placeholder = ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            opaque_provider_element_id: "uia-runtime:virtualized-placeholder:255".into(),
            semantic_locator_hints: vec!["name=LocalView Virtual Item 255".into()],
            parent_surface_ref: request.container_element_ref().parent_surface_ref.clone(),
            acquisition_cut_ref: request.snapshot_cut_ref().to_owned(),
            realization: ProviderElementRealization::RealizationRequired,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        };
        placeholder
            .semantic_locator_hints
            .push(format!("lookup={}", request.value()));
        Ok(WindowsUiaVirtualizedItemQueryReceipt {
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            container_element_ref: request.container_element_ref().clone(),
            placeholder_element_ref: placeholder,
        })
    }

    fn realize_virtualized_item(
        &self,
        _attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemRealizeRequest,
    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.realize_calls += 1;
        state.realized = true;
        Ok(WindowsUiaVirtualizedItemRealizeReceipt {
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            previous_placeholder_ref: request.placeholder_element_ref().clone(),
        })
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x5031)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x5032,
        expected_process_id: 503,
        selection_nonce: Uuid::from_u128(0x5033),
    }
}

#[tokio::test]
async fn realization_receipt_requires_a_fresh_runtime_reconciliation_cut_before_current_authority() {
    let provider = FakeProvider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        LiveBridge::new(64, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime.attach(session(), selection()).await.unwrap();

    let before = runtime
        .current_semantic_snapshot(session())
        .await
        .expect("initial runtime snapshot must exist");
    let container = before
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some("virtualized-list"))
        .expect("initial snapshot must expose exact virtualized container")
        .element_ref
        .clone();
    let query = WindowsUiaVirtualizedItemQueryRequest::new(
        before.snapshot_cut_ref(),
        container,
        WindowsUiaItemLookupProperty::Name,
        "LocalView Virtual Item 255",
    )
    .unwrap();

    let receipt = runtime
        .realize_virtualized_item_and_refresh(session(), query)
        .await
        .expect("runtime must realize under session authority and immediately reconcile a fresh cut");

    assert_eq!(
        receipt.query_receipt.placeholder_element_ref.realization,
        ProviderElementRealization::RealizationRequired
    );
    assert_eq!(
        receipt.realize_receipt.previous_placeholder_ref,
        receipt.query_receipt.placeholder_element_ref,
        "realization receipt must preserve the old non-actionable placeholder identity"
    );
    assert_ne!(receipt.fresh_snapshot.snapshot_cut_ref(), before.snapshot_cut_ref());
    assert!(
        receipt
            .fresh_snapshot
            .nodes()
            .iter()
            .all(|node| node.element_ref != receipt.query_receipt.placeholder_element_ref),
        "old placeholder identity must not survive into the fresh current snapshot"
    );
    let realized = receipt
        .fresh_snapshot
        .nodes()
        .iter()
        .find(|node| node.name.as_deref() == Some("LocalView Virtual Item 255"))
        .expect("fresh reconciliation must observe the newly realized item");
    assert_eq!(realized.element_ref.realization, ProviderElementRealization::RealizedCurrent);
    assert_eq!(
        realized.element_ref.acquisition_cut_ref,
        receipt.fresh_snapshot.snapshot_cut_ref()
    );

    let current = runtime
        .current_semantic_snapshot(session())
        .await
        .expect("fresh realization snapshot must become runtime current state");
    assert_eq!(current.observed_digest(), receipt.fresh_snapshot.observed_digest());
    assert_eq!(provider.counts(), (1, 1, 2));
}
