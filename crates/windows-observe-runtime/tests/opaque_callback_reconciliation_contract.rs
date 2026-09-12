use std::sync::{Arc, Mutex};

use chrono::Utc;
use localview_live_bridge::LiveBridge;
use localview_native_provider::{
    NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision, SemanticSnapshotCache,
    SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness, SessionId,
    TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage,
};
use localview_windows_uia_provider::{WindowsUiaEvent, WindowsUiaEventDrain, WindowsUiaEventKind};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Attachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct Subscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("opaque callback test provider failure")]
struct ProviderError;

#[derive(Debug, Clone)]
struct Provider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    snapshot_count: Arc<Mutex<u64>>,
    drained: Arc<Mutex<bool>>,
}

impl Provider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:opaque-callback"),
            target: TargetIncarnationRef::from("target:windows:opaque-callback"),
            snapshot_count: Arc::new(Mutex::new(0)),
            drained: Arc::new(Mutex::new(false)),
        }
    }

    fn snapshots(&self) -> u64 {
        *self.snapshot_count.lock().unwrap()
    }

    fn snapshot_revision(&self, sequence: u64, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:opaque-callback".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: sequence,
                nodes: vec![],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 0,
                    properties_read: 0,
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
        let mut drained = self.drained.lock().unwrap();
        if std::mem::replace(&mut *drained, true) {
            return Ok(WindowsUiaEventDrain {
                events: vec![],
                dropped_before_drain: 0,
                latest_sequence: 1,
            });
        }
        Ok(WindowsUiaEventDrain {
            events: vec![WindowsUiaEvent {
                sequence: 1,
                captured_at: Utc::now(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                kind: WindowsUiaEventKind::PropertyChanged { property_id: 30005 },
                element_ref: None,
            }],
            dropped_before_drain: 0,
            latest_sequence: 1,
        })
    }

    fn snapshot(
        &self,
        _attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        _surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        let sequence = {
            let mut count = self.snapshot_count.lock().unwrap();
            *count += 1;
            *count
        };
        Ok(self.snapshot_revision(sequence, snapshot_cut_ref))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn accepted_callback_under_opaque_ordering_forces_one_reconciliation_snapshot() {
    let provider = Provider::new();
    let manager = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        LiveBridge::new(64, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 8,
            drain_limit: 8,
        },
    )
    .unwrap();
    let session_id = SessionId::from_u128(0x4f01);

    let attached = manager
        .attach(
            session_id,
            UserSelectedWindowTarget {
                native_window_handle: 0x4f02,
                expected_process_id: 79,
                selection_nonce: Uuid::from_u128(0x4f03),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        attached.event_continuity,
        EventContinuityState::OrderingOpaque
    );
    assert_eq!(provider.snapshots(), 1, "attach owns one baseline snapshot");

    let outcome = manager.drain_once(session_id).await.unwrap();

    assert_eq!(outcome.report.ingest.accepted, 1);
    assert_eq!(
        outcome.report.continuity,
        EventContinuityState::OrderingOpaque
    );
    assert!(
        outcome.reconciliation_performed,
        "opaque best-effort callbacks cannot keep the pre-event snapshot authoritative"
    );
    assert_eq!(
        outcome.status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        provider.snapshots(),
        2,
        "one opaque callback causes exactly one fresh snapshot"
    );

    let quiet = manager.drain_once(session_id).await.unwrap();
    assert!(
        !quiet.reconciliation_performed,
        "no new callback means no global polling loop"
    );
    assert_eq!(provider.snapshots(), 2);
}
