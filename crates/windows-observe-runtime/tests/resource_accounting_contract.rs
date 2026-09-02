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
use localview_resource_governor::{ResourceWorkKind, RuntimeResourceGovernor};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveResourceAccounting, WindowsObserveRuntimeConfig,
    WindowsObserveRuntimeError, WindowsObserveRuntimeManager, WindowsObserveSubscriptionLineage,
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
#[error("fake resource-accounting provider failure")]
struct FakeError;

#[derive(Debug)]
struct FakeState {
    drains: VecDeque<WindowsUiaEventDrain>,
    attach_count: usize,
    subscribe_count: usize,
    snapshot_count: usize,
    drain_count: usize,
    pressure_on_next_drain: Option<RuntimeResourceGovernor>,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:resource-contract"),
            target: TargetIncarnationRef::from("target:windows:resource-contract"),
            state: Arc::new(Mutex::new(FakeState {
                drains: drains.into(),
                attach_count: 0,
                subscribe_count: 0,
                snapshot_count: 0,
                drain_count: 0,
                pressure_on_next_drain: None,
            })),
        }
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (
            state.attach_count,
            state.subscribe_count,
            state.snapshot_count,
            state.drain_count,
        )
    }

    fn arm_critical_pressure_on_next_drain(&self, governor: RuntimeResourceGovernor) {
        self.state.lock().unwrap().pressure_on_next_drain = Some(governor);
    }

    fn snapshot_revision(&self, sequence: u64) -> Arc<NativeSemanticSnapshotRevision> {
        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: format!("resource-cut:{sequence}"),
                surface_scope: "window:resource-contract".into(),
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
        self.state.lock().unwrap().attach_count += 1;
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
        self.state.lock().unwrap().subscribe_count += 1;
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
        let (drain, pressure) = {
            let mut state = self.state.lock().unwrap();
            state.drain_count += 1;
            (
                state.drains.pop_front().unwrap_or(WindowsUiaEventDrain {
                    events: vec![],
                    dropped_before_drain: 0,
                    latest_sequence: 0,
                }),
                state.pressure_on_next_drain.take(),
            )
        };
        if let Some(governor) = pressure {
            assert!(governor.update_process_metrics(1024, 100.0));
        }
        Ok(drain)
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
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4501)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x4502,
        expected_process_id: 45,
        selection_nonce: Uuid::from_u128(0x4503),
    }
}

fn event(provider: &FakeProvider, sequence: u64) -> WindowsUiaEvent {
    WindowsUiaEvent {
        sequence,
        captured_at: Utc::now(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        kind: WindowsUiaEventKind::FocusChanged,
        element_ref: None,
    }
}

fn manager(
    provider: FakeProvider,
    bridge: LiveBridge,
    governor: RuntimeResourceGovernor,
) -> WindowsObserveRuntimeManager<FakeProvider> {
    WindowsObserveRuntimeManager::with_resource_governor(
        Arc::new(provider),
        bridge,
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
        governor,
    )
    .unwrap()
}

#[tokio::test]
async fn critical_pressure_rejects_attach_before_any_provider_work() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    let governor = RuntimeResourceGovernor::default();
    assert!(governor.update_process_metrics(1024, 100.0));
    let manager = manager(provider.clone(), bridge, governor);

    let error = manager.attach(session(), selection()).await.unwrap_err();
    assert!(matches!(
        error,
        WindowsObserveRuntimeError::ResourceDenied {
            work_kind: ResourceWorkKind::NativeSemanticObservation,
            ..
        }
    ));
    assert_eq!(provider.counts(), (0, 0, 0, 0));
}

#[tokio::test]
async fn accounting_tracks_initial_snapshot_bounded_drains_drops_and_reconciliation() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    {
        let mut state = provider.state.lock().unwrap();
        state.drains.push_back(WindowsUiaEventDrain {
            events: vec![event(&provider, 1)],
            dropped_before_drain: 0,
            latest_sequence: 1,
        });
        state.drains.push_back(WindowsUiaEventDrain {
            events: vec![event(&provider, 4)],
            dropped_before_drain: 2,
            latest_sequence: 4,
        });
    }
    let manager = manager(
        provider.clone(),
        bridge,
        RuntimeResourceGovernor::default(),
    );

    manager.attach(session(), selection()).await.unwrap();
    manager.drain_once(session()).await.unwrap();
    let gap = manager.drain_once(session()).await.unwrap();
    assert!(gap.reconciliation_performed);

    assert_eq!(
        manager.resource_accounting(session()).await,
        Some(WindowsObserveResourceAccounting {
            initial_snapshots: 1,
            reconciliation_snapshots: 1,
            event_drains: 2,
            events_accepted: 2,
            events_rejected_stale: 0,
            provider_events_dropped: 2,
            snapshot_nodes_observed: 0,
            snapshot_properties_read: 0,
            snapshot_max_depth_observed: 0,
            incomplete_snapshots: 0,
            resource_denials: 0,
        })
    );
}

#[tokio::test]
async fn reconciliation_resource_denial_preserves_gap_debt_and_attachment_until_pressure_recovers() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new(vec![]);
    {
        let mut state = provider.state.lock().unwrap();
        state.drains.push_back(WindowsUiaEventDrain {
            events: vec![event(&provider, 1)],
            dropped_before_drain: 0,
            latest_sequence: 1,
        });
        state.drains.push_back(WindowsUiaEventDrain {
            events: vec![event(&provider, 4)],
            dropped_before_drain: 2,
            latest_sequence: 4,
        });
        state.drains.push_back(WindowsUiaEventDrain {
            events: vec![],
            dropped_before_drain: 0,
            latest_sequence: 4,
        });
    }
    let governor = RuntimeResourceGovernor::default();
    let manager = manager(provider.clone(), bridge, governor.clone());

    manager.attach(session(), selection()).await.unwrap();
    manager.drain_once(session()).await.unwrap();
    provider.arm_critical_pressure_on_next_drain(governor.clone());

    let error = manager.drain_once(session()).await.unwrap_err();
    assert!(matches!(
        error,
        WindowsObserveRuntimeError::ResourceDenied {
            work_kind: ResourceWorkKind::NativeSemanticReconciliation,
            ..
        }
    ));
    let status = manager.status(session()).await.unwrap();
    assert_eq!(status.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(status.current_snapshot_completeness, None);
    assert_eq!(provider.counts().2, 1, "denied reconciliation must not snapshot");

    assert!(governor.update_process_metrics(0, 0.0));
    let recovered = manager.drain_once(session()).await.unwrap();
    assert!(recovered.reconciliation_performed);
    assert_eq!(
        recovered.status.event_continuity,
        EventContinuityState::GapDetected
    );
    assert_eq!(
        recovered.status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(provider.counts().2, 2);

    let accounting = manager.resource_accounting(session()).await.unwrap();
    assert_eq!(accounting.resource_denials, 1);
    assert_eq!(accounting.provider_events_dropped, 2);
    assert_eq!(accounting.reconciliation_snapshots, 1);
}
