use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, LiveBridge, ObserverEvent,
    ObserverEventKind, ProviderObserverBatch,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    EventContinuityState, PrincipalRef, ProviderElementRealization, ProviderElementRef,
    ProviderIncarnationRef, ReconciliationCompleteness, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightRequest,
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
#[error("continuity test provider failure")]
struct ProviderError;

#[derive(Debug, Clone)]
struct Provider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
}

impl Provider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:preflight-continuity"),
            target: TargetIncarnationRef::from("target:windows:preflight-continuity"),
        }
    }

    fn snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Invoke, WindowsUiaPatternSupport::Supported);
        let mut attributes = BTreeMap::new();
        capabilities.write_attributes(&mut attributes);
        let element_ref = ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            opaque_provider_element_id: "uia-runtime:[91,1]".into(),
            semantic_locator_hints: vec!["automation_id=confirm".into()],
            parent_surface_ref: Some("window:continuity".into()),
            acquisition_cut_ref: cut.clone(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        };
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:continuity".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![NativeSemanticNodeObservation {
                    element_ref,
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

    fn subscription_lineage(&self, subscription: &Self::Subscription) -> WindowsObserveSubscriptionLineage {
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
async fn complete_cached_snapshot_cannot_preflight_across_unreconciled_event_gap() {
    let session_id = SessionId::from_u128(0x91d0);
    let provider = Provider::new();
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
                native_window_handle: 0x91d1,
                expected_process_id: 91,
                selection_nonce: Uuid::from_u128(0x91d2),
            },
        )
        .await
        .unwrap();
    let snapshot = runtime.current_semantic_snapshot(session_id).await.unwrap();

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            events: vec![ObserverEvent {
                seq: 2,
                captured_at: Utc::now(),
                kind: ObserverEventKind::SemanticSnapshot,
                reference: None,
                route: None,
                payload: serde_json::json!({"native_provider":"windows_uia"}),
            }],
        })
        .await;
    let status = runtime.status(session_id).await.unwrap();
    assert_eq!(status.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(status.current_snapshot_completeness, None);

    let result = runtime
        .preflight_uia_action(
            session_id,
            WindowsUiaActionPreflightRequest {
                authority: ActionEnvelopeMetadata {
                    decision_principal_ref: PrincipalRef::from("principal:decision:continuity"),
                    acting_principal_ref: PrincipalRef::from("principal:acting:continuity"),
                    authorization_revision: "authorization:continuity:v1".into(),
                    precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
                    provider_incarnation_ref: provider.provider,
                    target_incarnation_ref: provider.target,
                    risk_class: ActionRiskClass::DestructiveOrIrreversible,
                    idempotency_class: ActionIdempotencyClass::Irreversible,
                    expected_postcondition_contract_refs: vec!["postcondition:continuity".into()],
                },
                element_ref: snapshot.nodes()[0].element_ref.clone(),
                required_pattern: WindowsUiaPattern::Invoke,
            },
        )
        .await;

    assert!(
        result.is_err(),
        "a complete cached snapshot must not authorize preflight while current provider continuity has unreconciled debt"
    );
}
