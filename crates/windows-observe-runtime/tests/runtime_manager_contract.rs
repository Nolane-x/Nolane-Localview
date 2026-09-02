use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

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
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeError,
    WindowsObserveRuntimeManager, WindowsObserveSubscriptionLineage,
};
use localview_windows_uia_provider::{
    WindowsUiaEvent, WindowsUiaEventDrain, WindowsUiaEventKind,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment {
    target: TargetIncarnationRef,
}

#[derive(Debug, Clone)]
struct FakeSubscription {
    lineage: WindowsObserveSubscriptionLineage,
}

#[derive(Debug, Clone, Error)]
#[error("fake provider failure")]
struct FakeError;

#[derive(Debug)]
struct FakeState {
    drains: VecDeque<WindowsUiaEventDrain>,
    snapshot_count: usize,
    subscribe_count: usize,
    unsubscribe_count: usize,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new(drains: Vec<WindowsUiaEventDrain>) -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:manager-contract"),
            target: TargetIncarnationRef::from("target:windows:manager-contract"),
            state: Arc::new(Mutex::new(FakeState {
                drains: drains.into(),
                snapshot_count: 0,
                subscribe_count: 0,
                unsubscribe_count: 0,
            })),
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (
            state.snapshot_count,
            state.subscribe_count,
            state.unsubscribe_count,
        )
    }

    fn snapshot_revision(&self, sequence: u64) -> Arc<NativeSemanticSnapshotRevision> {
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: format!("uia-manager-cut:{sequence}"),
                surface_scope: "window:manager-contract".into(),
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

impl WindowsObserveProvider for FakeProvider {
    type Attachment = FakeAttachment;
    type Subscription = FakeSubscription;
    type Error = FakeError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.provider.clone()
    }

    fn attach(
        &self,
        _selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error> {
        Ok(FakeAttachment {
            target: self.target.clone(),
        })
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.target.clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        assert!(capacity > 0);
        let mut state = self.state.lock().unwrap();
        state.subscribe_count += 1;
        Ok(FakeSubscription {
            lineage: WindowsObserveSubscriptionLineage {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: attachment.target.clone(),
                sequence_baseline: 0,
            },
        })
    }

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage {
        subscription.lineage.clone()
    }

    fn drain_events(
        &self,
        _subscription: &Self::Subscription,
        limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error> {
        assert!(limit > 0);
        Ok(self
            .state
            .lock()
            .unwrap()
            .drains
            .pop_front()
            .unwrap_or(WindowsUiaEventDrain {
                events: vec![],
                dropped_before_drain: 0,
                latest_sequence: 0,
            }))
    }

    fn snapshot(
        &self,
        _attachment: &Self::Attachment,
        _snapshot_cut_ref: String,
        _surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        let sequence = {
            let mut state = self.state.lock().unwrap();
            state.snapshot_count += 1;
            state.snapshot_count as u64
        };
        Ok(self.snapshot_revision(sequence))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        self.state.lock().unwrap().unsubscribe_count += 1;
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4401)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x1234,
        expected_process_id: 77,
        selection_nonce: Uuid::from_u128(0x4402),
    }
}

#[tokio::test]
async fn attach_owns_subscription_binds_opaque_lineage_and_establishes_initial_snapshot() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    let manager = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 32,
            drain_limit: 16,
        },
    )
    .unwrap();

    let status = manager.attach(session(), selection()).await.unwrap();

    assert_eq!(status.generation, 1);
    assert_eq!(status.provider_incarnation_ref, provider.provider);
    assert_eq!(status.target_incarnation_ref, provider.target);
    assert_eq!(status.event_continuity, EventContinuityState::OrderingOpaque);
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(provider.counts(), (1, 1, 0));
}

#[tokio::test]
async fn contiguous_callback_drain_does_not_poll_snapshot_but_gap_reconciles_once() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![
        WindowsUiaEventDrain {
            events: vec![event_placeholder(1)],
            dropped_before_drain: 0,
            latest_sequence: 1,
        },
        WindowsUiaEventDrain {
            events: vec![event_placeholder(4)],
            dropped_before_drain: 2,
            latest_sequence: 4,
        },
    ]);
    let provider = rewrite_drain_lineage(provider);
    let manager = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge,
        WindowsObserveRuntimeConfig {
            event_capacity: 32,
            drain_limit: 16,
        },
    )
    .unwrap();
    manager.attach(session(), selection()).await.unwrap();
    assert_eq!(
        provider.counts().0,
        1,
        "attach establishes exactly one snapshot baseline"
    );

    let first = manager.drain_once(session()).await.unwrap();
    assert!(!first.reconciliation_performed);
    assert_eq!(
        provider.counts().0,
        1,
        "contiguous callbacks must not trigger UI tree polling"
    );

    let second = manager.drain_once(session()).await.unwrap();
    assert!(second.reconciliation_performed);
    assert_eq!(
        second.status.event_continuity,
        EventContinuityState::GapDetected
    );
    assert_eq!(
        second.status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        provider.counts().0,
        2,
        "one observed gap causes one bounded reconciliation snapshot"
    );

    let third = manager.drain_once(session()).await.unwrap();
    assert!(!third.reconciliation_performed);
    assert_eq!(
        third.status.event_continuity,
        EventContinuityState::GapDetected
    );
    assert_eq!(
        third.status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        provider.counts().0,
        2,
        "reconciled historical gap must not trigger repeat snapshots"
    );
}

#[tokio::test]
async fn duplicate_attach_is_rejected_and_release_unsubscribes_and_clears_provider_binding() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    let manager = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 8,
            drain_limit: 4,
        },
    )
    .unwrap();

    manager.attach(session(), selection()).await.unwrap();
    assert!(matches!(
        manager.attach(session(), selection()).await.unwrap_err(),
        WindowsObserveRuntimeError::AlreadyAttached { .. }
    ));

    manager.release(session()).await.unwrap();
    assert!(manager.status(session()).await.is_none());
    assert!(bridge.observation_status(session()).await.is_none());
    assert_eq!(provider.counts(), (1, 1, 1));
}

#[tokio::test]
async fn invalid_runtime_bounds_fail_closed_before_provider_work() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    let error = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge,
        WindowsObserveRuntimeConfig {
            event_capacity: 0,
            drain_limit: 4,
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        WindowsObserveRuntimeError::InvalidConfiguration
    ));
    assert_eq!(provider.counts(), (0, 0, 0));
}

fn event_placeholder(sequence: u64) -> WindowsUiaEvent {
    WindowsUiaEvent {
        sequence,
        captured_at: Utc::now(),
        provider_incarnation_ref: ProviderIncarnationRef::from("placeholder-provider"),
        target_incarnation_ref: TargetIncarnationRef::from("placeholder-target"),
        kind: WindowsUiaEventKind::FocusChanged,
        element_ref: None,
    }
}

fn rewrite_drain_lineage(provider: FakeProvider) -> FakeProvider {
    let mut state = provider.state.lock().unwrap();
    for drain in &mut state.drains {
        for event in &mut drain.events {
            event.provider_incarnation_ref = provider.provider.clone();
            event.target_incarnation_ref = provider.target.clone();
        }
    }
    drop(state);
    provider
}
