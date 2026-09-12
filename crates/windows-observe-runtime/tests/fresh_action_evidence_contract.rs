use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use localview_live_bridge::LiveBridge;
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotBudgetLimit, SnapshotResourceUsage, UserSelectedWindowTarget,
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
    WindowsUiaActionCapabilities, WindowsUiaEventDrain, WindowsUiaPattern, WindowsUiaPatternSupport,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FreshSnapshotMode {
    #[default]
    Present,
    Missing,
    Duplicate,
    Incomplete,
}

#[derive(Debug, Default)]
struct ProviderState {
    snapshots: usize,
    fresh_mode: FreshSnapshotMode,
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

    fn set_fresh_mode(&self, mode: FreshSnapshotMode) {
        self.state.lock().unwrap().fresh_mode = mode;
    }

    fn semantic_node(&self, cut: &str) -> NativeSemanticNodeObservation {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(
            WindowsUiaPattern::Invoke,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::new();
        capabilities.write_attributes(&mut attributes);

        NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[92,1]".into(),
                semantic_locator_hints: vec!["automation_id=confirm".into()],
                parent_surface_ref: Some("window:fresh-action-evidence".into()),
                acquisition_cut_ref: cut.to_owned(),
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
        }
    }

    fn snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let (capture_sequence, mode) = {
            let mut state = self.state.lock().unwrap();
            state.snapshots += 1;
            (
                state.snapshots as u64,
                if state.snapshots == 1 {
                    FreshSnapshotMode::Present
                } else {
                    state.fresh_mode
                },
            )
        };

        let node = self.semantic_node(&cut);
        let nodes = match mode {
            FreshSnapshotMode::Present | FreshSnapshotMode::Incomplete => vec![node],
            FreshSnapshotMode::Missing => Vec::new(),
            FreshSnapshotMode::Duplicate => vec![node.clone(), node],
        };
        let incomplete = mode == FreshSnapshotMode::Incomplete;
        let completeness = if incomplete {
            ReconciliationCompleteness::Incomplete
        } else {
            ReconciliationCompleteness::Established
        };
        let exhausted = if incomplete {
            vec![SnapshotBudgetLimit::Nodes]
        } else {
            Vec::new()
        };
        let incompleteness_debt = if incomplete {
            vec!["snapshot_budget_nodes_exhausted".into()]
        } else {
            Vec::new()
        };

        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:fresh-action-evidence".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence,
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: nodes.len(),
                    properties_read: nodes.len().saturating_mul(14),
                    max_depth_observed: 0,
                    exhausted,
                    incomplete,
                },
                nodes,
                completeness,
                incompleteness_debt,
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

    fn attach(
        &self,
        _selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error> {
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

async fn fixture(
    session_id: SessionId,
) -> (
    Provider,
    WindowsObserveRuntimeManager<Provider>,
    Arc<NativeSemanticSnapshotRevision>,
    ProviderElementRef,
) {
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
    (provider, runtime, before, previous_element_ref)
}

#[tokio::test]
async fn refresh_action_evidence_captures_a_new_cut_and_rebinds_the_exact_provider_element() {
    let session_id = SessionId::from_u128(0x9201);
    let (provider, runtime, before, previous_element_ref) = fixture(session_id).await;
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

#[tokio::test]
async fn stale_requested_element_is_rejected_before_any_provider_refresh() {
    let session_id = SessionId::from_u128(0x9210);
    let (provider, runtime, before, mut previous_element_ref) = fixture(session_id).await;
    previous_element_ref.acquisition_cut_ref = "cut:stale:caller".into();

    assert!(
        runtime
            .refresh_uia_action_evidence(session_id, previous_element_ref)
            .await
            .is_err()
    );
    assert_eq!(provider.snapshot_count(), 1);
    assert_eq!(
        runtime
            .current_semantic_snapshot(session_id)
            .await
            .unwrap()
            .snapshot_cut_ref(),
        before.snapshot_cut_ref()
    );
}

#[tokio::test]
async fn missing_element_fails_binding_but_preserves_the_fresh_complete_world_revision() {
    let session_id = SessionId::from_u128(0x9220);
    let (provider, runtime, before, previous_element_ref) = fixture(session_id).await;
    provider.set_fresh_mode(FreshSnapshotMode::Missing);

    assert!(
        runtime
            .refresh_uia_action_evidence(session_id, previous_element_ref)
            .await
            .is_err()
    );
    assert_eq!(provider.snapshot_count(), 2);

    let current = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_ne!(current.snapshot_cut_ref(), before.snapshot_cut_ref());
    assert!(current.nodes().is_empty());
    assert_eq!(
        current.completeness(),
        ReconciliationCompleteness::Established,
        "complete fresh absence is current world evidence even though action binding fails"
    );
    let status = runtime.status(session_id).await.unwrap();
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert!(status.reconciliation_receipt_id.is_some());
}

#[tokio::test]
async fn duplicate_provider_identity_fails_binding_but_preserves_the_fresh_revision() {
    let session_id = SessionId::from_u128(0x9230);
    let (provider, runtime, before, previous_element_ref) = fixture(session_id).await;
    provider.set_fresh_mode(FreshSnapshotMode::Duplicate);

    assert!(
        runtime
            .refresh_uia_action_evidence(session_id, previous_element_ref)
            .await
            .is_err()
    );

    let current = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_ne!(current.snapshot_cut_ref(), before.snapshot_cut_ref());
    assert_eq!(current.nodes().len(), 2);
    assert_eq!(
        current.completeness(),
        ReconciliationCompleteness::Established
    );
}

#[tokio::test]
async fn incomplete_fresh_snapshot_is_published_as_current_evidence_but_never_authorizes_binding() {
    let session_id = SessionId::from_u128(0x9240);
    let (provider, runtime, before, previous_element_ref) = fixture(session_id).await;
    provider.set_fresh_mode(FreshSnapshotMode::Incomplete);

    assert!(
        runtime
            .refresh_uia_action_evidence(session_id, previous_element_ref)
            .await
            .is_err()
    );

    let current = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_ne!(current.snapshot_cut_ref(), before.snapshot_cut_ref());
    assert_eq!(
        current.completeness(),
        ReconciliationCompleteness::Incomplete
    );
    assert!(current.resource_usage().incomplete);
    assert!(!current.incompleteness_debt().is_empty());
    let status = runtime.status(session_id).await.unwrap();
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Incomplete)
    );
    assert!(status.reconciliation_receipt_id.is_some());
}
