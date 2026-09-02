use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    ConsequentialJournal, LiveBridge,
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
    seal_uia_dispatch, WindowsObserveActionLeaseProvider, WindowsObserveDispatchContextProvider,
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightRequest,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaDispatchSealError, WindowsUiaDispatchSealRequest,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextReceipt,
    WindowsUiaDispatchContextRequest, WindowsUiaDispatchContextRequirements,
    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,
    WindowsUiaPattern, WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake dispatch seal provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    lease_calls: usize,
    context_calls: usize,
    forge_context_element: bool,
    forge_context_requirements: bool,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:dispatch-seal"),
            target: TargetIncarnationRef::from("target:windows:dispatch-seal"),
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

    fn context_calls(&self) -> usize {
        self.state.lock().unwrap().context_calls
    }

    fn forge_next_context_element(&self) {
        self.state.lock().unwrap().forge_context_element = true;
    }

    fn forge_next_context_requirements(&self) {
        self.state.lock().unwrap().forge_context_requirements = true;
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
                opaque_provider_element_id: "uia-runtime:[81,1]".into(),
                semantic_locator_hints: vec!["automation_id=dispatch-seal".into()],
                parent_surface_ref: Some("window:dispatch-seal".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Enable sealed dispatch".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("dispatch-seal".into()),
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
                surface_scope: "window:dispatch-seal".into(),
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
            .expect("lease binding requires current snapshot");
        if request.snapshot_cut_ref != snapshot.snapshot_cut_ref()
            || request.element_ref != snapshot.nodes()[0].element_ref
        {
            return Err(FakeError);
        }
        Ok(WindowsUiaElementLeaseReceipt {
            snapshot_cut_ref: request.snapshot_cut_ref,
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            element_ref: request.element_ref,
        })
    }
}

impl WindowsObserveDispatchContextProvider for FakeProvider {
    fn revalidate_dispatch_context(
        &self,
        _attachment: &Self::Attachment,
        request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaBoundDispatchContextReceipt, Self::Error> {
        let mut state = self.state.lock().unwrap();
        state.context_calls += 1;
        let mut element_ref = request.element_ref.clone();
        if state.forge_context_element {
            element_ref.opaque_provider_element_id = "uia-runtime:[forged-context]".into();
            state.forge_context_element = false;
        }
        let mut requirements = request.requirements;
        if state.forge_context_requirements {
            requirements.require_exact_element_focus = !requirements.require_exact_element_focus;
            state.forge_context_requirements = false;
        }

        Ok(WindowsUiaBoundDispatchContextReceipt {
            requirements,
            context: WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                element_ref,
                observation: WindowsUiaDispatchContextObservation {
                    target_window_handle: 0x8102,
                    target_process_id: 81,
                    foreground_window_handle: Some(0x8102),
                    foreground_process_id: Some(81),
                    exact_element_focused: request
                        .requirements
                        .require_exact_element_focus
                        .then_some(true),
                    modal_blocker_window_handle: None,
                },
            },
        })
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake authorization authority failure")]
struct FakeAuthorizationError;

#[derive(Debug, Clone)]
struct FakeAuthorizationRevalidator;

impl WindowsUiaAuthorizationRevalidator for FakeAuthorizationRevalidator {
    type Error = FakeAuthorizationError;

    fn revalidate(
        &self,
        action_id: Uuid,
        authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
        Ok(WindowsUiaAuthorizationRevalidationReceipt {
            action_id,
            decision_principal_ref: authority.decision_principal_ref.clone(),
            acting_principal_ref: authority.acting_principal_ref.clone(),
            authorization_revision: authority.authorization_revision.clone(),
        })
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x8101)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x8102,
        expected_process_id: 81,
        selection_nonce: Uuid::from_u128(0x8103),
    }
}

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-windows-{label}-{}.jsonl", Uuid::new_v4()))
}

fn authority(provider: &FakeProvider, snapshot: &NativeSemanticSnapshotRevision) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:dispatch-seal"),
        acting_principal_ref: PrincipalRef::from("principal:acting:dispatch-seal"),
        authorization_revision: "authorization:dispatch-seal:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:dispatch-seal".into()],
    }
}

fn requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: true,
        require_no_modal_blocker: true,
    }
}

async fn fixture(
    label: &str,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    FakeProvider,
    WindowsObserveRuntimeManager<FakeProvider>,
    WindowsUiaActionPreflightRequest,
    Uuid,
) {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime.attach(session(), selection()).await.unwrap();
    let snapshot = provider.snapshot();
    let metadata = authority(&provider, &snapshot);
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Focus, metadata.clone())
        .await
        .unwrap();

    let path = journal_path(label);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();

    (
        bridge,
        journal,
        path,
        provider,
        runtime,
        WindowsUiaActionPreflightRequest {
            authority: metadata,
            element_ref: snapshot.nodes()[0].element_ref.clone(),
            required_pattern: WindowsUiaPattern::Toggle,
        },
        queued.action.id,
    )
}

#[tokio::test]
async fn seal_revalidates_semantics_authority_journal_and_exact_provider_context() {
    let (bridge, journal, path, provider, runtime, preflight_request, action_id) =
        fixture("dispatch-seal-success").await;
    let authority = preflight_request.authority.clone();
    let preflight = runtime
        .preflight_uia_action(session(), preflight_request)
        .await
        .unwrap();

    let receipt = seal_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaDispatchSealRequest {
            action_id,
            authority: authority.clone(),
            preflight: preflight.clone(),
            context_requirements: requirements(),
        },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();

    assert_eq!(receipt.authority.action_id, action_id);
    assert_eq!(receipt.authority.authority, authority);
    assert_eq!(receipt.authority.dispatch_revalidation.preflight, preflight);
    assert_eq!(receipt.context.requirements, requirements());
    assert_eq!(
        receipt.context.snapshot_cut_ref,
        receipt.authority.dispatch_revalidation.element_lease.snapshot_cut_ref
    );
    assert_eq!(
        receipt.context.element_ref,
        receipt.authority.dispatch_revalidation.element_lease.element_ref
    );
    assert_eq!(provider.context_calls(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn forged_context_element_or_requirement_binding_fails_closed() {
    let (bridge, journal, path, provider, runtime, preflight_request, action_id) =
        fixture("dispatch-seal-forged-context").await;
    let authority = preflight_request.authority.clone();
    let preflight = runtime
        .preflight_uia_action(session(), preflight_request)
        .await
        .unwrap();
    provider.forge_next_context_element();

    assert_eq!(
        seal_uia_dispatch(
            &bridge,
            &journal,
            &runtime,
            session(),
            WindowsUiaDispatchSealRequest {
                action_id,
                authority: authority.clone(),
                preflight: preflight.clone(),
                context_requirements: requirements(),
            },
            &FakeAuthorizationRevalidator,
        )
        .await
        .unwrap_err(),
        WindowsUiaDispatchSealError::ContextReceiptMismatch
    );

    let (bridge2, journal2, path2, provider2, runtime2, preflight_request2, action_id2) =
        fixture("dispatch-seal-forged-requirements").await;
    let authority2 = preflight_request2.authority.clone();
    let preflight2 = runtime2
        .preflight_uia_action(session(), preflight_request2)
        .await
        .unwrap();
    provider2.forge_next_context_requirements();

    assert_eq!(
        seal_uia_dispatch(
            &bridge2,
            &journal2,
            &runtime2,
            session(),
            WindowsUiaDispatchSealRequest {
                action_id: action_id2,
                authority: authority2,
                preflight: preflight2,
                context_requirements: requirements(),
            },
            &FakeAuthorizationRevalidator,
        )
        .await
        .unwrap_err(),
        WindowsUiaDispatchSealError::ContextReceiptMismatch
    );

    assert_eq!(provider.context_calls(), 1);
    assert_eq!(provider2.context_calls(), 1);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path2);
}
