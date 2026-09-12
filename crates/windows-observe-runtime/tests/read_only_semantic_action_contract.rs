use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    LiveBridge,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotBudgetLimit, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    PrincipalRef, ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsSemanticReadError, WindowsSemanticReadRequest,
};
use localview_windows_uia_provider::{WindowsUiaEvent, WindowsUiaEventDrain};
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
#[error("fake read-only provider failure")]
struct FakeError;

#[derive(Debug)]
struct FakeState {
    drains: VecDeque<WindowsUiaEventDrain>,
    snapshots: Vec<Arc<NativeSemanticSnapshotRevision>>,
    snapshot_count: usize,
    drain_count: usize,
    next_snapshot_incomplete: bool,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:read-only-contract"),
            target: TargetIncarnationRef::from("target:windows:read-only-contract"),
            state: Arc::new(Mutex::new(FakeState {
                drains: VecDeque::new(),
                snapshots: Vec::new(),
                snapshot_count: 0,
                drain_count: 0,
                next_snapshot_incomplete: false,
            })),
        }
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.snapshot_count, state.drain_count)
    }

    fn latest_snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshots
            .last()
            .cloned()
            .expect("snapshot must exist")
    }

    fn push_gap_drain(&self) {
        self.state.lock().unwrap().drains.push_back(WindowsUiaEventDrain {
            events: vec![WindowsUiaEvent {
                sequence: 4,
                captured_at: chrono::Utc::now(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                kind: localview_windows_uia_provider::WindowsUiaEventKind::FocusChanged,
                element_ref: None,
            }],
            dropped_before_drain: 3,
            latest_sequence: 4,
        });
    }

    fn make_next_snapshot_incomplete(&self) {
        self.state.lock().unwrap().next_snapshot_incomplete = true;
    }

    fn publish_snapshot(
        &self,
        sequence: u64,
        cut: String,
        incomplete: bool,
    ) -> Arc<NativeSemanticSnapshotRevision> {
        let element_ref = ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            opaque_provider_element_id: format!("uia-runtime:[42,{sequence}]"),
            semantic_locator_hints: vec!["automation_id=save".into()],
            parent_surface_ref: Some("window:read-only-contract".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        };
        let node = NativeSemanticNodeObservation {
            element_ref,
            parent_index: None,
            depth: 0,
            role: Some("button".into()),
            name: Some(format!("Save {sequence}")),
            control_type: Some("uia_control_type:50000".into()),
            automation_id: Some("save".into()),
            class_name: Some("Button".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::from([("provider".into(), "windows_uia".into())]),
        };
        let usage = if incomplete {
            SnapshotResourceUsage {
                nodes_observed: 1,
                properties_read: 7,
                max_depth_observed: 0,
                exhausted: vec![SnapshotBudgetLimit::Nodes],
                incomplete: true,
            }
        } else {
            SnapshotResourceUsage {
                nodes_observed: 1,
                properties_read: 7,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: false,
            }
        };
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:read-only-contract".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: sequence,
                nodes: vec![node],
                resource_usage: usage,
                completeness: if incomplete {
                    ReconciliationCompleteness::Incomplete
                } else {
                    ReconciliationCompleteness::Established
                },
                incompleteness_debt: if incomplete {
                    vec!["snapshot_budget_exhausted:Nodes".into()]
                } else {
                    vec![]
                },
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
        _capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
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
        _limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.drain_count += 1;
        Ok(state.drains.pop_front().unwrap_or(WindowsUiaEventDrain {
            events: vec![],
            dropped_before_drain: 0,
            latest_sequence: 0,
        }))
    }

    fn snapshot(
        &self,
        _attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        _surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        let (sequence, incomplete) = {
            let mut state = self.state.lock().unwrap();
            state.snapshot_count += 1;
            let incomplete = state.next_snapshot_incomplete;
            state.next_snapshot_incomplete = false;
            (state.snapshot_count as u64, incomplete)
        };
        let snapshot = self.publish_snapshot(sequence, snapshot_cut_ref, incomplete);
        self.state.lock().unwrap().snapshots.push(snapshot.clone());
        Ok(snapshot)
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4601)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x4602,
        expected_process_id: 46,
        selection_nonce: Uuid::from_u128(0x4603),
    }
}

fn manager(
    provider: FakeProvider,
    bridge: LiveBridge,
) -> WindowsObserveRuntimeManager<FakeProvider> {
    WindowsObserveRuntimeManager::new(
        Arc::new(provider),
        bridge,
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap()
}

fn authority(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:read-only"),
        acting_principal_ref: PrincipalRef::from("principal:acting:read-only"),
        authorization_revision: "authorization:read-only:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ObserveOnly,
        idempotency_class: ActionIdempotencyClass::PureRead,
        expected_postcondition_contract_refs: vec![],
    }
}

fn request(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> WindowsSemanticReadRequest {
    WindowsSemanticReadRequest {
        authority: authority(provider, snapshot),
        element_ref: snapshot.nodes()[0].element_ref.clone(),
    }
}

#[tokio::test]
async fn exact_current_snapshot_read_returns_normalized_node_without_provider_or_action_work() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let manager = manager(provider.clone(), bridge.clone());
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();
    let before = provider.counts();

    let receipt = manager
        .read_semantic(session(), request(&provider, &snapshot))
        .await
        .unwrap();

    assert_eq!(receipt.snapshot_cut_ref, snapshot.snapshot_cut_ref());
    assert_eq!(receipt.cache_revision_ref, snapshot.cache_revision_ref());
    assert_eq!(receipt.observed_digest, snapshot.observed_digest());
    assert_eq!(receipt.node, snapshot.nodes()[0]);
    assert_eq!(provider.counts(), before, "pure read must not call provider or refresh UIA");
    assert!(bridge.take_public_actions(session(), 8).await.is_empty());
}

#[tokio::test]
async fn read_requires_observe_only_pure_read_canonical_authority() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let manager = manager(provider.clone(), bridge);
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();

    let mut wrong_risk = request(&provider, &snapshot);
    wrong_risk.authority.risk_class = ActionRiskClass::ReversibleUiState;
    assert!(matches!(
        manager.read_semantic(session(), wrong_risk).await.unwrap_err(),
        WindowsSemanticReadError::ObserveOnlyRiskRequired
    ));

    let mut wrong_idempotency = request(&provider, &snapshot);
    wrong_idempotency.authority.idempotency_class = ActionIdempotencyClass::IdempotentByObservedState;
    assert!(matches!(
        manager
            .read_semantic(session(), wrong_idempotency)
            .await
            .unwrap_err(),
        WindowsSemanticReadError::PureReadIdempotencyRequired
    ));
}

#[tokio::test]
async fn stale_snapshot_cut_and_stale_element_are_rejected_after_reconciliation() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let manager = manager(provider.clone(), bridge);
    manager.attach(session(), selection()).await.unwrap();
    let old_snapshot = provider.latest_snapshot();
    let old_request = request(&provider, &old_snapshot);

    provider.push_gap_drain();
    let outcome = manager.drain_once(session()).await.unwrap();
    assert!(outcome.reconciliation_performed);
    let current_snapshot = provider.latest_snapshot();
    assert_ne!(old_snapshot.snapshot_cut_ref(), current_snapshot.snapshot_cut_ref());

    assert!(matches!(
        manager
            .read_semantic(session(), old_request.clone())
            .await
            .unwrap_err(),
        WindowsSemanticReadError::PreconditionSnapshotCutMismatch { .. }
    ));

    let mut stale_element = request(&provider, &current_snapshot);
    stale_element.element_ref = old_request.element_ref;
    assert!(matches!(
        manager
            .read_semantic(session(), stale_element)
            .await
            .unwrap_err(),
        WindowsSemanticReadError::ElementAcquisitionCutMismatch { .. }
    ));
}

#[tokio::test]
async fn provider_or_target_authority_mismatch_reuses_canonical_binding_errors() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let manager = manager(provider.clone(), bridge);
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();

    let mut wrong_provider = request(&provider, &snapshot);
    wrong_provider.authority.provider_incarnation_ref =
        ProviderIncarnationRef::from("provider:windows-uia:stale");
    assert_eq!(
        manager.read_semantic(session(), wrong_provider).await.unwrap_err(),
        WindowsSemanticReadError::Authority(ActionEnvelopeBindingError::ProviderIncarnationMismatch)
    );

    let mut wrong_target = request(&provider, &snapshot);
    wrong_target.authority.target_incarnation_ref =
        TargetIncarnationRef::from("target:windows:stale");
    assert_eq!(
        manager.read_semantic(session(), wrong_target).await.unwrap_err(),
        WindowsSemanticReadError::Authority(ActionEnvelopeBindingError::TargetIncarnationMismatch)
    );
}

#[tokio::test]
async fn incomplete_snapshot_cannot_silently_authorize_a_semantic_read() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    provider.make_next_snapshot_incomplete();
    let manager = manager(provider.clone(), bridge);
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();
    assert_eq!(snapshot.completeness(), ReconciliationCompleteness::Incomplete);

    assert!(matches!(
        manager
            .read_semantic(session(), request(&provider, &snapshot))
            .await
            .unwrap_err(),
        WindowsSemanticReadError::SnapshotIncomplete
    ));
}

#[tokio::test]
async fn exact_cut_but_unknown_element_fails_closed_without_fuzzy_locator_fallback() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let manager = manager(provider.clone(), bridge);
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();
    let mut unknown = request(&provider, &snapshot);
    unknown.element_ref.opaque_provider_element_id = "uia-runtime:[999,999]".into();
    unknown.element_ref.semantic_locator_hints = snapshot.nodes()[0]
        .element_ref
        .semantic_locator_hints
        .clone();

    assert!(matches!(
        manager.read_semantic(session(), unknown).await.unwrap_err(),
        WindowsSemanticReadError::ElementNotFound
    ));
}
