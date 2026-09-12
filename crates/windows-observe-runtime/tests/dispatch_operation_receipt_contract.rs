use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, ConsequentialJournal, ConsequentialJournalTransition,
    ConsequentialRecoveryState, LiveBridge,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderElementRealization, ProviderElementRef,
    ProviderIncarnationRef, ReconciliationCompleteness, SessionId, TargetIncarnationRef,
    TransportResult,
};
use localview_windows_observe_runtime::{
    WindowsObserveActionLeaseProvider, WindowsObserveDispatchContextProvider,
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightRequest,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutor,
    WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchRequest,
    WindowsUiaProviderExecutionReceipt, WindowsUiaProviderExecutionRequest,
    arm_uia_dispatch_execution, execute_armed_uia_dispatch, prepare_uia_dispatch,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextReceipt,
    WindowsUiaDispatchContextRequest, WindowsUiaDispatchContextRequirements,
    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,
    WindowsUiaPattern, WindowsUiaPatternDispatchOperation, WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake provider failure")]
struct FakeProviderError;

#[derive(Debug, Default)]
struct FakeProviderState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    state: Arc<Mutex<FakeProviderState>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from(
                "provider:windows-uia:dispatch-operation-receipt",
            ),
            target: TargetIncarnationRef::from("target:windows:dispatch-operation-receipt"),
            state: Arc::new(Mutex::new(FakeProviderState::default())),
        }
    }

    fn snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("runtime must publish an initial snapshot")
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(
            WindowsUiaPattern::ExpandCollapse,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[402,1]".into(),
                semantic_locator_hints: vec!["automation_id=expandable".into()],
                parent_surface_ref: Some("window:dispatch-operation-receipt".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("tree item".into()),
            name: Some("Expandable".into()),
            control_type: Some("uia_control_type:50024".into()),
            automation_id: Some("expandable".into()),
            class_name: Some("TreeViewItem".into()),
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
                surface_scope: "window:dispatch-operation-receipt".into(),
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
    type Error = FakeProviderError;

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
        let snapshot = self.snapshot();
        if request.snapshot_cut_ref != snapshot.snapshot_cut_ref()
            || request.element_ref != snapshot.nodes()[0].element_ref
        {
            return Err(FakeProviderError);
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
        Ok(WindowsUiaBoundDispatchContextReceipt {
            requirements: request.requirements,
            context: WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                element_ref: request.element_ref,
                observation: WindowsUiaDispatchContextObservation {
                    target_window_handle: 0x4020,
                    target_process_id: 402,
                    foreground_window_handle: Some(0x4020),
                    foreground_process_id: Some(402),
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

#[derive(Debug)]
struct ForgeDispatchOperationExecutor;

impl WindowsUiaDispatchExecutor for ForgeDispatchOperationExecutor {
    type Error = FakeProviderError;

    async fn execute(
        &self,
        request: &WindowsUiaProviderExecutionRequest,
    ) -> Result<WindowsUiaProviderExecutionReceipt, Self::Error> {
        assert_eq!(
            request.dispatch_operation(),
            WindowsUiaPatternDispatchOperation::Expand,
            "durably admitted Expand must mint an Expand provider verb"
        );

        Ok(WindowsUiaProviderExecutionReceipt {
            dispatch_attempt_ref: request.dispatch_attempt_ref(),
            action_id: request.action_id(),
            preparation_journal_sequence: request.preparation_journal_sequence(),
            preparation_receipt_ref: request.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref: request.element_ref().clone(),
            required_pattern: request.required_pattern(),
            dispatch_operation: WindowsUiaPatternDispatchOperation::Collapse,
            context_requirements: request.context_requirements(),
            transport_result: TransportResult::DeliveredToExecutor,
            dispatch_result: DispatchResult::DispatchedFull,
        })
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4021)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x4020,
        expected_process_id: 402,
        selection_nonce: Uuid::from_u128(0x4022),
    }
}

fn requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: true,
        require_no_modal_blocker: true,
    }
}

fn authority(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:dispatch-operation-receipt"),
        acting_principal_ref: PrincipalRef::from("principal:acting:dispatch-operation-receipt"),
        authorization_revision: "authorization:dispatch-operation-receipt:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec![
            "postcondition:dispatch-operation-receipt".into(),
        ],
    }
}

fn journal_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-windows-dispatch-operation-receipt-{}.jsonl",
        Uuid::new_v4()
    ))
}

#[tokio::test]
async fn forged_dispatch_operation_receipt_is_rejected_before_linearization() {
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
    let metadata = authority(&provider, snapshot.as_ref());
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Click, metadata.clone())
        .await
        .unwrap();

    let path = journal_path();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal
        .record_intent_operation_bound_explicit(&queued, CanonicalActionOperation::Expand)
        .await
        .unwrap();

    let preflight = runtime
        .preflight_uia_action(
            session(),
            WindowsUiaActionPreflightRequest {
                authority: metadata.clone(),
                element_ref: snapshot.nodes()[0].element_ref.clone(),
                required_pattern: WindowsUiaPattern::ExpandCollapse,
            },
        )
        .await
        .unwrap();

    let prepared = prepare_uia_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        WindowsUiaPreparedDispatchRequest {
            seal: WindowsUiaDispatchSealRequest {
                action_id: queued.action.id,
                authority: metadata,
                preflight,
                context_requirements: requirements(),
            },
        },
        &FakeAuthorizationRevalidator,
    )
    .await
    .unwrap();
    let armed = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap();
    let action_id = armed.action_id();

    let error = execute_armed_uia_dispatch(
        &bridge,
        &journal,
        session(),
        armed,
        &ForgeDispatchOperationExecutor,
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptMismatch
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));
    assert!(
        journal
            .entries_for(action_id)
            .await
            .iter()
            .all(|entry| !matches!(
                entry.transition,
                ConsequentialJournalTransition::DispatchLinearized { .. }
            )),
        "forged operation receipt must never be linearized"
    );

    let _ = std::fs::remove_file(path);
}
