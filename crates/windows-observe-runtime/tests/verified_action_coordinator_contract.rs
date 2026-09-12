use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    ConsequentialJournal, ConsequentialPostconditionEvidence, ConsequentialPostconditionStatus,
    ConsequentialRecoveryState, LiveBridge,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderElementRealization, ProviderElementRef,
    ProviderIncarnationRef, ReconciliationCompleteness, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
use localview_windows_observe_runtime::{
    WindowsObserveActionLeaseProvider, WindowsObserveDispatchContextProvider,
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaAuthorizationRevalidationReceipt,
    WindowsUiaAuthorizationRevalidator, WindowsUiaDispatchExecutor, WindowsUiaPostconditionVerifier,
    WindowsUiaProviderExecutionReceipt, WindowsUiaProviderExecutionRequest,
    WindowsUiaVerifiedActionTarget, WindowsUiaVerifiedExecutionOutcome,
    execute_verified_canonical_uia_action,
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
#[error("fake provider failure")]
struct FakeProviderError;

#[derive(Debug, Default)]
struct FakeProviderState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    context_calls: usize,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:verified-action-coordinator"),
            target: TargetIncarnationRef::from("target:windows:verified-action-coordinator"),
            state: Arc::new(Mutex::new(FakeProviderState::default())),
        }
    }

    fn current_snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("runtime must publish an initial snapshot")
    }

    fn context_calls(&self) -> usize {
        self.state.lock().unwrap().context_calls
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(
            WindowsUiaPattern::Invoke,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[120,1]".into(),
                semantic_locator_hints: vec!["automation_id=verified-action".into()],
                parent_surface_ref: Some("window:verified-action".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("button".into()),
            name: Some("Verified Action".into()),
            control_type: Some("uia_control_type:50000".into()),
            automation_id: Some("verified-action".into()),
            class_name: Some("Button".into()),
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
                surface_scope: "window:verified-action".into(),
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
        let snapshot = self.current_snapshot();
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
        self.state.lock().unwrap().context_calls += 1;
        Ok(WindowsUiaBoundDispatchContextReceipt {
            requirements: request.requirements,
            context: WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                element_ref: request.element_ref,
                observation: WindowsUiaDispatchContextObservation {
                    target_window_handle: 0x1200,
                    target_process_id: 120,
                    foreground_window_handle: Some(0x1200),
                    foreground_process_id: Some(120),
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

#[derive(Default)]
struct FakeAuthorizationRevalidator {
    calls: Mutex<usize>,
}

impl FakeAuthorizationRevalidator {
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl WindowsUiaAuthorizationRevalidator for FakeAuthorizationRevalidator {
    type Error = FakeAuthorizationError;

    fn revalidate(
        &self,
        action_id: Uuid,
        authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
        *self.calls.lock().unwrap() += 1;
        Ok(WindowsUiaAuthorizationRevalidationReceipt {
            action_id,
            decision_principal_ref: authority.decision_principal_ref.clone(),
            acting_principal_ref: authority.acting_principal_ref.clone(),
            authorization_revision: authority.authorization_revision.clone(),
        })
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake executor failure")]
struct FakeExecutorError;

#[derive(Default)]
struct FakeExecutor {
    calls: Mutex<usize>,
}

impl FakeExecutor {
    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl WindowsUiaDispatchExecutor for FakeExecutor {
    type Error = FakeExecutorError;

    async fn execute(
        &self,
        request: &WindowsUiaProviderExecutionRequest,
    ) -> Result<WindowsUiaProviderExecutionReceipt, Self::Error> {
        *self.calls.lock().unwrap() += 1;
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
            dispatch_operation: request.dispatch_operation(),
            context_requirements: request.context_requirements(),
            transport_result: TransportResult::DeliveredToExecutor,
            dispatch_result: DispatchResult::DispatchedFull,
        })
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake verifier failure")]
struct FakeVerifierError;

struct FakeVerifier;

impl WindowsUiaPostconditionVerifier for FakeVerifier {
    type Error = FakeVerifierError;

    fn verify(
        &self,
        _action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        Ok(expected_contract_refs
            .iter()
            .map(|contract_ref| ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status: ConsequentialPostconditionStatus::VerifiedPass,
                receipt_ref: format!("verified-action:{}:{contract_ref}", snapshot.snapshot_cut_ref()),
            })
            .collect())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x1201)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x1200,
        expected_process_id: 120,
        selection_nonce: Uuid::from_u128(0x1202),
    }
}

fn authority(provider: &FakeProvider, snapshot: &NativeSemanticSnapshotRevision) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:verified-action"),
        acting_principal_ref: PrincipalRef::from("principal:acting:verified-action"),
        authorization_revision: "authorization:verified-action:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:verified-action".into()],
    }
}

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

#[tokio::test]
async fn one_call_coordinator_closes_exact_canonical_action_through_verified_commit() {
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

    let snapshot = provider.current_snapshot();
    let metadata = authority(&provider, &snapshot);
    let queued = bridge
        .enqueue_canonical_action(session(), None, BridgeActionKind::Click, metadata)
        .await
        .unwrap();
    let path = journal_path("verified-action-coordinator-success");
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal.record_intent_operation_bound(&queued).await.unwrap();

    let authorization = FakeAuthorizationRevalidator::default();
    let executor = FakeExecutor::default();
    let outcome = execute_verified_canonical_uia_action(
        &bridge,
        &journal,
        &runtime,
        &queued,
        WindowsUiaVerifiedActionTarget {
            element_ref: snapshot.nodes()[0].element_ref.clone(),
            required_pattern: WindowsUiaPattern::Invoke,
            context_requirements: WindowsUiaDispatchContextRequirements {
                require_foreground_target: true,
                require_exact_element_focus: false,
                require_no_modal_blocker: true,
            },
        },
        &authorization,
        &executor,
        &FakeVerifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::Committed {
            world_outcome: WorldOutcome::VerifiedExpected,
            ..
        }
    ));
    assert_eq!(authorization.call_count(), 1);
    assert_eq!(executor.call_count(), 1);
    assert_eq!(provider.context_calls(), 2);
    assert_eq!(
        journal.recovery_state(queued.action.id).await,
        Some(ConsequentialRecoveryState::Committed)
    );

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!(
        "{}.operation-{}.json",
        path.display(),
        queued.action.id
    ));
}
