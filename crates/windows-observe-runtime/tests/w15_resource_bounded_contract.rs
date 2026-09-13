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
#[error("W15 provider failure")]
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:w15-resource-bounded"),
            target: TargetIncarnationRef::from("target:windows:w15-resource-bounded"),
            state: Arc::new(Mutex::new(ProviderState::default())),
        }
    }

    fn semantic_node(&self, cut: &str) -> NativeSemanticNodeObservation {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Invoke, WindowsUiaPatternSupport::Supported);
        let mut attributes = BTreeMap::new();
        capabilities.write_attributes(&mut attributes);

        NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[15,1]".into(),
                semantic_locator_hints: vec!["automation_id=w15-action".into()],
                parent_surface_ref: Some("window:w15-resource-bounded".into()),
                acquisition_cut_ref: cut.to_owned(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("button".into()),
            name: Some("W15 Action".into()),
            control_type: Some("uia_control_type:50000".into()),
            automation_id: Some("w15-action".into()),
            class_name: Some("Button".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes,
        }
    }

    fn snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let capture_sequence = {
            let mut state = self.state.lock().unwrap();
            state.snapshots += 1;
            state.snapshots as u64
        };
        let incomplete = capture_sequence > 1;
        let nodes = vec![self.semantic_node(&cut)];
        let exhausted = if incomplete {
            vec![SnapshotBudgetLimit::Nodes]
        } else {
            Vec::new()
        };
        let incompleteness_debt = if incomplete {
            vec!["snapshot_budget_exhausted:Nodes".into()]
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
                surface_scope: "window:w15-resource-bounded".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence,
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: nodes.len(),
                    properties_read: nodes.len().saturating_mul(19),
                    max_depth_observed: 0,
                    exhausted,
                    incomplete,
                },
                nodes,
                completeness: if incomplete {
                    ReconciliationCompleteness::Incomplete
                } else {
                    ReconciliationCompleteness::Established
                },
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
async fn w15_resource_bounded_reconciliation_is_current_but_never_action_authority() {
    let session_id = SessionId::from_u128(0x1501);
    let provider = Provider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider),
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
                native_window_handle: 0x1502,
                expected_process_id: 15,
                selection_nonce: Uuid::from_u128(0x1503),
            },
        )
        .await
        .unwrap();

    let before = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_eq!(
        before.completeness(),
        ReconciliationCompleteness::Established
    );
    let previous_element_ref = before.nodes()[0].element_ref.clone();

    let error = runtime
        .refresh_uia_action_evidence(session_id, previous_element_ref)
        .await
        .expect_err("resource-bounded reconciliation must not mint fresh action authority");

    let current = runtime.current_semantic_snapshot(session_id).await.unwrap();
    assert_ne!(current.snapshot_cut_ref(), before.snapshot_cut_ref());
    assert_eq!(current.completeness(), ReconciliationCompleteness::Incomplete);
    assert!(current.resource_usage().incomplete);
    assert_eq!(
        current.resource_usage().exhausted,
        vec![SnapshotBudgetLimit::Nodes]
    );
    assert!(
        current
            .incompleteness_debt()
            .iter()
            .any(|debt| debt == "snapshot_budget_exhausted:Nodes")
    );

    let status = runtime.status(session_id).await.unwrap();
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Incomplete)
    );
    assert!(status.reconciliation_receipt_id.is_some());

    assert!(
        error.to_string().contains("resource-bounded"),
        "W15 requires a typed resource-bounded outcome, got: {error}"
    );
}
