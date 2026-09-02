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
    WindowsObserveActionLeaseProvider, WindowsObserveProvider, WindowsObserveRuntimeConfig,
    WindowsObserveRuntimeManager, WindowsObserveSubscriptionLineage,
    WindowsUiaActionPreflightRequest, WindowsUiaDispatchRevalidationError,
    WindowsUiaDispatchRevalidationRequest,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
    WindowsUiaEventDrain, WindowsUiaPattern, WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake dispatch revalidation provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    lease_calls: usize,
    forge_lease_receipt: bool,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:dispatch-revalidation"),
            target: TargetIncarnationRef::from("target:windows:dispatch-revalidation"),
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("attached runtime publishes an initial snapshot")
    }

    fn lease_calls(&self) -> usize {
        self.state.lock().unwrap().lease_calls
    }

    fn forge_next_lease_receipt(&self) {
        self.state.lock().unwrap().forge_lease_receipt = true;
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(
            WindowsUiaPattern::Toggle,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[50,1]".into(),
                semantic_locator_hints: vec!["automation_id=feature-toggle".into()],
                parent_surface_ref: Some("window:dispatch-revalidation".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Enable feature".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("feature-toggle".into()),
            class_name: Some("Button".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes,
        };

        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:dispatch-revalidation".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![node],
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

impl WindowsObserveActionLeaseProvider for FakeProvider {
    fn bind_element_lease(
        &self,
        _attachment: &Self::Attachment,
        request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.lease_calls += 1;
        let snapshot = state
            .snapshot
            .as_ref()
            .expect("lease binding requires a current snapshot");
        if request.snapshot_cut_ref != snapshot.snapshot_cut_ref()
            || request.element_ref != snapshot.nodes()[0].element_ref
        {
            return Err(FakeError);
        }

        let mut element_ref = request.element_ref;
        if state.forge_lease_receipt {
            element_ref.opaque_provider_element_id = "uia-runtime:[forged]".into();
            state.forge_lease_receipt = false;
        }

        Ok(WindowsUiaElementLeaseReceipt {
            snapshot_cut_ref: request.snapshot_cut_ref,
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            element_ref,
        })
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x5001)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x5002,
        expected_process_id: 50,
        selection_nonce: Uuid::from_u128(0x5003),
    }
}

fn build_manager(provider: FakeProvider) -> WindowsObserveRuntimeManager<FakeProvider> {
    WindowsObserveRuntimeManager::new(
        Arc::new(provider),
        LiveBridge::new(64, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap()
}

fn preflight_request(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> WindowsUiaActionPreflightRequest {
    WindowsUiaActionPreflightRequest {
        authority: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:dispatch-revalidation"),
            acting_principal_ref: PrincipalRef::from("principal:acting:dispatch-revalidation"),
            authorization_revision: "authorization:dispatch-revalidation:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:toggle-state".into()],
        },
        element_ref: snapshot.nodes()[0].element_ref.clone(),
        required_pattern: WindowsUiaPattern::Toggle,
    }
}

#[tokio::test]
async fn exact_preflight_authority_revalidates_and_binds_one_exact_worker_lease() {
    let provider = FakeProvider::new();
    let runtime = build_manager(provider.clone());
    runtime.attach(session(), selection()).await.unwrap();
    let snapshot = provider.snapshot();
    let request = preflight_request(&provider, &snapshot);
    let authority = request.authority.clone();
    let preflight = runtime
        .preflight_uia_action(session(), request)
        .await
        .unwrap();

    assert_eq!(preflight.authority, authority);
    assert_eq!(provider.lease_calls(), 0, "preflight remains provider-read free");

    let receipt = runtime
        .revalidate_uia_dispatch(
            session(),
            WindowsUiaDispatchRevalidationRequest {
                authority: authority.clone(),
                preflight: preflight.clone(),
            },
        )
        .await
        .unwrap();

    assert_eq!(receipt.authority, authority);
    assert_eq!(receipt.preflight, preflight);
    assert_eq!(receipt.element_lease.snapshot_cut_ref, snapshot.snapshot_cut_ref());
    assert_eq!(receipt.element_lease.provider_incarnation_ref, provider.provider);
    assert_eq!(receipt.element_lease.target_incarnation_ref, provider.target);
    assert_eq!(receipt.element_lease.element_ref, snapshot.nodes()[0].element_ref);
    assert_eq!(provider.lease_calls(), 1);
}

#[tokio::test]
async fn mutated_authority_or_stale_preflight_fails_before_live_lease_binding() {
    let provider = FakeProvider::new();
    let runtime = build_manager(provider.clone());
    runtime.attach(session(), selection()).await.unwrap();
    let snapshot = provider.snapshot();
    let request = preflight_request(&provider, &snapshot);
    let authority = request.authority.clone();
    let preflight = runtime
        .preflight_uia_action(session(), request)
        .await
        .unwrap();

    let mut mutated = authority.clone();
    mutated.acting_principal_ref = PrincipalRef::from("principal:acting:other");
    assert_eq!(
        runtime
            .revalidate_uia_dispatch(
                session(),
                WindowsUiaDispatchRevalidationRequest {
                    authority: mutated,
                    preflight: preflight.clone(),
                },
            )
            .await
            .unwrap_err(),
        WindowsUiaDispatchRevalidationError::PreflightAuthorityMismatch
    );
    assert_eq!(provider.lease_calls(), 0);

    runtime.release(session()).await.unwrap();
    runtime.attach(session(), selection()).await.unwrap();

    assert!(matches!(
        runtime
            .revalidate_uia_dispatch(
                session(),
                WindowsUiaDispatchRevalidationRequest {
                    authority,
                    preflight,
                },
            )
            .await
            .unwrap_err(),
        WindowsUiaDispatchRevalidationError::Preflight(_)
    ));
    assert_eq!(provider.lease_calls(), 0, "stale semantic evidence must fail before lease binding");
}

#[tokio::test]
async fn forged_worker_lease_receipt_is_rejected_fail_closed() {
    let provider = FakeProvider::new();
    let runtime = build_manager(provider.clone());
    runtime.attach(session(), selection()).await.unwrap();
    let snapshot = provider.snapshot();
    let request = preflight_request(&provider, &snapshot);
    let authority = request.authority.clone();
    let preflight = runtime
        .preflight_uia_action(session(), request)
        .await
        .unwrap();
    provider.forge_next_lease_receipt();

    assert_eq!(
        runtime
            .revalidate_uia_dispatch(
                session(),
                WindowsUiaDispatchRevalidationRequest {
                    authority,
                    preflight,
                },
            )
            .await
            .unwrap_err(),
        WindowsUiaDispatchRevalidationError::LeaseReceiptMismatch
    );
    assert_eq!(provider.lease_calls(), 1);
}
