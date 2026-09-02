use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState, LiveBridge,
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
    arm_uia_dispatch_execution, prepare_uia_dispatch, WindowsObserveActionLeaseProvider,
    WindowsObserveDispatchContextProvider, WindowsObserveProvider, WindowsObserveRuntimeConfig,
    WindowsObserveRuntimeManager, WindowsObserveSubscriptionLineage,
    WindowsUiaActionPreflightRequest, WindowsUiaAuthorizationRevalidationReceipt,
    WindowsUiaAuthorizationRevalidator, WindowsUiaDispatchExecutionArmError,
    WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchError,
    WindowsUiaPreparedDispatchRequest,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextBlocker, WindowsUiaDispatchContextObservation,
    WindowsUiaDispatchContextReceipt, WindowsUiaDispatchContextRequest,
    WindowsUiaDispatchContextRequirements, WindowsUiaElementLeaseReceipt,
    WindowsUiaElementLeaseRequest, WindowsUiaEventDrain, WindowsUiaPattern,
    WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake prepared dispatch provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    lease_calls: usize,
    context_calls: usize,
    forge_context_on_call: Option<usize>,
    block_focus_on_call: Option<usize>,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:prepared-dispatch"),
            target: TargetIncarnationRef::from("target:windows:prepared-dispatch"),
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

    fn forge_context_on_call(&self, call: usize) {
        self.state.lock().unwrap().forge_context_on_call = Some(call);
    }

    fn block_focus_on_call(&self, call: usize) {
        self.state.lock().unwrap().block_focus_on_call = Some(call);
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Toggle, WindowsUiaPatternSupport::Supported);
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[91,1]".into(),
                semantic_locator_hints: vec!["automation_id=prepared-dispatch".into()],
                parent_surface_ref: Some("window:prepared-dispatch".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Prepared dispatch".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("prepared-dispatch".into()),
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
                surface_scope: "window:prepared-dispatch".into(),
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
        let snapshot = state.snapshot.as_ref().expect("current snapshot required");
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
        let (call, forge_context, block_focus) = {
            let mut state = self.state.lock().unwrap();
            state.context_calls += 1;
            let call = state.context_calls;
            (
                call,
                state.forge_context_on_call == Some(call),
                state.block_focus_on_call == Some(call),
            )
        };

        let mut element_ref = request.element_ref;
        if forge_context {
            element_ref.opaque_provider_element_id = format!("uia-runtime:[91,forged-{call}]");
        }

        Ok(WindowsUiaBoundDispatchContextReceipt {
            requirements: request.requirements,
            context: WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                element_ref,
                observation: WindowsUiaDispatchContextObservation {
                    target_window_handle: 0x9102,
                    target_process_id: 91,
                    foreground_window_handle: Some(0x9102),
                    foreground_process_id: Some(91),
                    exact_element_focused: request
                        .requirements
                        .require_exact_element_focus
                        .then_some(!block_focus),
                    modal_blocker_window_handle: None,
                },
            },
        })
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake authorization failure")]
struct FakeAuthorizationError;

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
    Uuid::from_u128(0x9101)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x9102,
        expected_process_id: 91,
        selection_nonce: Uuid::from_u128(0x9103),
    }
}

fn requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: true,
        require_no_modal_blocker: true,
    }
}

fn authority(provider: &FakeProvider, snapshot: &NativeSemanticSnapshotRevision) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:prepared-dispatch"),
        acting_principal_ref: PrincipalRef::from("principal:acting:prepared-dispatch"),
        authorization_revision: "authorization:prepared-dispatch:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:prepared-dispatch".into()],
    }
}

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-windows-{label}-{}.jsonl", Uuid::new_v4()))
}

async fn fixture(
    label: &str,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    FakeProvider,
    WindowsObserveRuntimeManager<FakeProvider>,
    WindowsUiaDispatchSealRequest,
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

    let preflight = runtime
        .preflight_uia_action(
            session(),
            WindowsUiaActionPreflightRequest {
                authority: metadata.clone(),
                element_ref: snapshot.nodes()[0].element_ref.clone(),
                required_pattern: WindowsUiaPattern::Toggle,
            },
        )
        .await
        .unwrap();

    (
        bridge,
        journal,
        path,
        provider,
        runtime,
        WindowsUiaDispatchSealRequest {
            action_id: queued.action.id,
            authority: metadata,
            preflight,
            context_requirements: requirements(),
        },
    )
}

#[tokio::test]
async fn sealed_dispatch_is_durably_prepared_before_any_executor_boundary() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("prepared-dispatch-success").await;
    let action_id = seal_request.action_id;

    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();

    assert_eq!(prepared.action_id(), action_id);
    assert_eq!(
        prepared.preparation().authorization_journal_sequence,
        prepared.seal().authority.authorization_journal_sequence
    );
    assert_eq!(
        prepared.preparation().precondition_snapshot_cut_ref,
        prepared.seal().authority.authority.precondition_snapshot_cut_ref
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(provider.context_calls(), 1);

    let entries = journal.entries_for(action_id).await;
    assert!(matches!(
        entries.last().map(|entry| &entry.transition),
        Some(ConsequentialJournalTransition::DispatchPrepared { receipt })
            if receipt == prepared.preparation()
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_second_prepare_attempt_fails_closed_without_a_second_context_grant() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("prepared-dispatch-duplicate").await;
    let duplicate_request = seal_request.clone();

    let _prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();
    assert_eq!(provider.context_calls(), 1);

    let error = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest {
            seal: duplicate_request,
        },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap_err();

    assert!(matches!(error, WindowsUiaPreparedDispatchError::Seal(_)));
    assert_eq!(provider.context_calls(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn execution_arm_reobserves_exact_context_before_consuming_prepared_capability() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("execution-arm-success").await;
    let action_id = seal_request.action_id;

    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();
    assert_eq!(provider.context_calls(), 1);

    let armed = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap();

    assert_eq!(armed.action_id(), action_id);
    assert_eq!(provider.context_calls(), 2);
    assert_eq!(
        armed.armed_context().requirements,
        armed.seal().context.requirements
    );
    assert_eq!(
        armed.armed_context().element_ref,
        armed.seal().authority.dispatch_revalidation.element_lease.element_ref
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(
        journal.requires_reconciliation(action_id).await,
        Some(true)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn execution_arm_rejects_second_context_element_drift_and_never_reprepares() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("execution-arm-forged-context").await;
    let action_id = seal_request.action_id;
    let duplicate_request = seal_request.clone();
    provider.forge_context_on_call(2);

    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();
    assert_eq!(provider.context_calls(), 1);

    let error = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap_err();
    assert_eq!(error, WindowsUiaDispatchExecutionArmError::ContextReceiptMismatch);
    assert_eq!(provider.context_calls(), 2);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));

    let retry = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest {
            seal: duplicate_request,
        },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap_err();
    assert!(matches!(retry, WindowsUiaPreparedDispatchError::Seal(_)));
    assert_eq!(provider.context_calls(), 2);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn execution_arm_independently_rejects_blocked_focus_even_when_receipt_binding_matches() {
    let (bridge, journal, path, provider, runtime, seal_request) =
        fixture("execution-arm-blocked-focus").await;
    let action_id = seal_request.action_id;
    provider.block_focus_on_call(2);

    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest { seal: seal_request },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();

    let error = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        WindowsUiaDispatchExecutionArmError::ContextBlocked(
            WindowsUiaDispatchContextBlocker::ExactElementFocusMismatch
        )
    );
    assert_eq!(provider.context_calls(), 2);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));

    let _ = std::fs::remove_file(path);
}
