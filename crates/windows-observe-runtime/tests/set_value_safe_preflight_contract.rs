use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, LiveBridge,
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
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightError,
    WindowsUiaActionPreflightRequest,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBooleanCapabilityFact, WindowsUiaEventDrain,
    WindowsUiaPattern, WindowsUiaPatternSupport, WindowsUiaValueCapabilityFacts,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake SetValue preflight provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    is_password: WindowsUiaBooleanCapabilityFact,
    is_read_only: WindowsUiaBooleanCapabilityFact,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new(
        is_password: WindowsUiaBooleanCapabilityFact,
        is_read_only: WindowsUiaBooleanCapabilityFact,
    ) -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:set-value-preflight"),
            target: TargetIncarnationRef::from("target:windows:set-value-preflight"),
            is_password,
            is_read_only,
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("initial snapshot exists")
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(
            WindowsUiaPattern::Value,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::new();
        capabilities.write_attributes(&mut attributes);
        WindowsUiaValueCapabilityFacts::new(
            WindowsUiaPatternSupport::Supported,
            self.is_password,
            self.is_read_only,
        )
        .write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[91,7]".into(),
                semantic_locator_hints: vec!["automation_id=set-value-input".into()],
                parent_surface_ref: Some("window:set-value-preflight".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("edit".into()),
            name: Some("Set value input".into()),
            control_type: Some("uia_control_type:50004".into()),
            automation_id: Some("set-value-input".into()),
            class_name: Some("Edit".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes,
        };

        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:set-value-preflight".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![node],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 1,
                    properties_read: 16,
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
        self.state.lock().unwrap().snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x9171)
}

async fn attached(provider: &FakeProvider) -> WindowsObserveRuntimeManager<FakeProvider> {
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        LiveBridge::new(32, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime
        .attach(
            session(),
            UserSelectedWindowTarget {
                native_window_handle: 0x9172,
                expected_process_id: 91,
                selection_nonce: Uuid::from_u128(0x9173),
            },
        )
        .await
        .unwrap();
    runtime
}

fn request(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> WindowsUiaActionPreflightRequest {
    WindowsUiaActionPreflightRequest {
        authority: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:set-value-preflight"),
            acting_principal_ref: PrincipalRef::from("principal:acting:set-value-preflight"),
            authorization_revision: "authorization:set-value-preflight:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::DestructiveOrIrreversible,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![
                "lvpc:payload-equality:v1:{\"mode\":\"replace_value\",\"payload_ref\":\"00000000-0000-0000-0000-000000009174\"}".into(),
            ],
        },
        element_ref: snapshot.nodes()[0].element_ref.clone(),
        required_pattern: WindowsUiaPattern::Value,
    }
}

#[tokio::test]
async fn value_preflight_accepts_only_explicit_non_password_writable_facts() {
    let safe = FakeProvider::new(
        WindowsUiaBooleanCapabilityFact::False,
        WindowsUiaBooleanCapabilityFact::False,
    );
    let runtime = attached(&safe).await;
    let snapshot = safe.snapshot();
    assert!(
        runtime
            .preflight_uia_action(session(), request(&safe, &snapshot))
            .await
            .is_ok()
    );

    for (is_password, is_read_only) in [
        (
            WindowsUiaBooleanCapabilityFact::True,
            WindowsUiaBooleanCapabilityFact::False,
        ),
        (
            WindowsUiaBooleanCapabilityFact::Unknown,
            WindowsUiaBooleanCapabilityFact::False,
        ),
        (
            WindowsUiaBooleanCapabilityFact::False,
            WindowsUiaBooleanCapabilityFact::True,
        ),
        (
            WindowsUiaBooleanCapabilityFact::False,
            WindowsUiaBooleanCapabilityFact::Unknown,
        ),
    ] {
        let provider = FakeProvider::new(is_password, is_read_only);
        let runtime = attached(&provider).await;
        let snapshot = provider.snapshot();
        assert_eq!(
            runtime
                .preflight_uia_action(session(), request(&provider, &snapshot))
                .await
                .unwrap_err(),
            WindowsUiaActionPreflightError::SetValueUnsafeCapability {
                is_password,
                is_read_only,
            },
        );
    }
}
