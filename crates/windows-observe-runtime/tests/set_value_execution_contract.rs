use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    CanonicalActionOperation, ConsequentialJournal, ConsequentialRecoveryState, LiveBridge,
    SetValueCommitmentKey, SetValueMode, SetValuePayloadRef,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_postcondition_contracts::{
    PayloadEqualityModeV1, PayloadEqualityPostconditionContractV1,
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
    WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchRequest,
    WindowsUiaSetValueExecutionPayload, WindowsUiaSetValueExecutor,
    WindowsUiaSetValueVerificationRequest, WindowsUiaVerifiedExecutionOutcome,
    arm_uia_dispatch_execution, execute_armed_uia_set_value_dispatch, prepare_uia_dispatch,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextReceipt,
    WindowsUiaDispatchContextRequest, WindowsUiaDispatchContextRequirements,
    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,
    WindowsUiaPattern, WindowsUiaPatternSupport, WindowsUiaSetValueDispatchReceipt,
    WindowsUiaSetValueDispatchRequest, WindowsUiaSetValueEquality,
    WindowsUiaSetValueVerificationReceipt,
};
use thiserror::Error;
use uuid::Uuid;

const EXPECTED_VALUE: &[u8] = b"set-value-equality-sentinel";

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
            provider: ProviderIncarnationRef::from("provider:windows-uia:set-value-execution"),
            target: TargetIncarnationRef::from("target:windows:set-value-execution"),
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
            WindowsUiaPattern::Value,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([
            ("provider".into(), "windows_uia".into()),
            ("windows_uia.is_password".into(), "false".into()),
            ("windows_uia.value.is_read_only".into(), "false".into()),
        ]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[107,1]".into(),
                semantic_locator_hints: vec!["automation_id=set-value-execution".into()],
                parent_surface_ref: Some("window:set-value-execution".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("edit".into()),
            name: Some("SetValue execution".into()),
            control_type: Some("uia_control_type:50004".into()),
            automation_id: Some("set-value-execution".into()),
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
                surface_scope: "window:set-value-execution".into(),
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
                    target_window_handle: 0x1070,
                    target_process_id: 107,
                    foreground_window_handle: Some(0x1070),
                    foreground_process_id: Some(107),
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

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake SetValue executor failure")]
struct FakeExecutorError;

#[derive(Debug)]
struct FakeSetValueExecutor {
    equality: WindowsUiaSetValueEquality,
    dispatch_calls: Mutex<usize>,
    verification_calls: Mutex<usize>,
}

impl FakeSetValueExecutor {
    fn new(equality: WindowsUiaSetValueEquality) -> Self {
        Self {
            equality,
            dispatch_calls: Mutex::new(0),
            verification_calls: Mutex::new(0),
        }
    }

    fn dispatch_calls(&self) -> usize {
        *self.dispatch_calls.lock().unwrap()
    }

    fn verification_calls(&self) -> usize {
        *self.verification_calls.lock().unwrap()
    }
}

impl WindowsUiaSetValueExecutor for FakeSetValueExecutor {
    type Error = FakeExecutorError;

    async fn dispatch_set_value(
        &self,
        request: WindowsUiaSetValueDispatchRequest,
    ) -> Result<WindowsUiaSetValueDispatchReceipt, Self::Error> {
        *self.dispatch_calls.lock().unwrap() += 1;
        Ok(WindowsUiaSetValueDispatchReceipt {
            dispatch_attempt_ref: request.dispatch_attempt_ref,
            action_id: request.action_id,
            preparation_journal_sequence: request.preparation_journal_sequence,
            preparation_receipt_ref: request.preparation_receipt_ref,
            snapshot_cut_ref: request.snapshot_cut_ref,
            provider_incarnation_ref: request.provider_incarnation_ref,
            target_incarnation_ref: request.target_incarnation_ref,
            element_ref: request.element_ref,
            required_pattern: WindowsUiaPattern::Value,
            dispatch_operation: CanonicalActionOperation::SetValue,
            payload_ref: request.payload_ref,
            mode: request.mode,
            context_requirements: request.context_requirements,
            final_context: WindowsUiaDispatchContextObservation {
                target_window_handle: 0x1070,
                target_process_id: 107,
                foreground_window_handle: Some(0x1070),
                foreground_process_id: Some(107),
                exact_element_focused: Some(true),
                modal_blocker_window_handle: None,
            },
            transport_result: TransportResult::DeliveredToExecutor,
            dispatch_result: DispatchResult::DispatchedFull,
        })
    }

    async fn verify_set_value(
        &self,
        request: &WindowsUiaSetValueVerificationRequest<'_>,
    ) -> Result<WindowsUiaSetValueVerificationReceipt, Self::Error> {
        *self.verification_calls.lock().unwrap() += 1;
        assert_eq!(request.expected_utf8(), EXPECTED_VALUE);
        Ok(WindowsUiaSetValueVerificationReceipt {
            action_id: request.action_id(),
            payload_ref: request.payload_ref(),
            mode: request.mode(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref: request.element_ref().clone(),
            observation_cut_ref: request.observation_cut_ref().to_owned(),
            equality: self.equality,
        })
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x1071)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x1070,
        expected_process_id: 107,
        selection_nonce: Uuid::from_u128(0x1072),
    }
}

fn requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: true,
        require_no_modal_blocker: true,
    }
}

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-windows-set-value-{label}-{}.jsonl",
        Uuid::new_v4()
    ))
}

async fn prepared_and_armed(
    label: &str,
    payload_ref: SetValuePayloadRef,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    WindowsObserveRuntimeManager<FakeProvider>,
    localview_windows_observe_runtime::WindowsUiaDispatchExecutionPermit,
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
    let expected_contract = PayloadEqualityPostconditionContractV1 {
        mode: PayloadEqualityModeV1::ReplaceValue,
        payload_ref: payload_ref.0.to_string(),
    }
    .to_contract_ref()
    .unwrap();
    let metadata = ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:set-value-execution"),
        acting_principal_ref: PrincipalRef::from("principal:acting:set-value-execution"),
        authorization_revision: "authorization:set-value-execution:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::DestructiveOrIrreversible,
        idempotency_class: ActionIdempotencyClass::Irreversible,
        expected_postcondition_contract_refs: vec![expected_contract],
    };
    let queued = bridge
        .bind_direct_canonical_action(
            session(),
            None,
            BridgeActionKind::TypeText {
                text: String::new(),
                clear_first: false,
            },
            metadata.clone(),
        )
        .await
        .unwrap();

    let path = journal_path(label);
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    journal
        .record_intent_admitted(queued.envelope.clone())
        .await
        .unwrap();
    journal
        .record_intent_operation_bound_explicit(&queued, CanonicalActionOperation::SetValue)
        .await
        .unwrap();
    let key = SetValueCommitmentKey::generate().unwrap();
    journal
        .record_set_value_payload_binding(
            &queued,
            &key,
            payload_ref,
            SetValueMode::ReplaceValue,
            EXPECTED_VALUE,
        )
        .await
        .unwrap();

    let preflight = runtime
        .preflight_uia_action(
            session(),
            WindowsUiaActionPreflightRequest {
                authority: metadata.clone(),
                element_ref: snapshot.nodes()[0].element_ref.clone(),
                required_pattern: WindowsUiaPattern::Value,
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
    (bridge, journal, path, runtime, armed)
}

async fn run_case(
    label: &str,
    equality: WindowsUiaSetValueEquality,
) -> (
    WindowsUiaVerifiedExecutionOutcome,
    ConsequentialRecoveryState,
    usize,
    usize,
) {
    let payload_ref = SetValuePayloadRef(Uuid::new_v4());
    let (bridge, journal, path, runtime, armed) = prepared_and_armed(label, payload_ref).await;
    let executor = FakeSetValueExecutor::new(equality);
    let outcome = execute_armed_uia_set_value_dispatch(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        WindowsUiaSetValueExecutionPayload {
            payload_ref,
            mode: SetValueMode::ReplaceValue,
            utf8_bytes: EXPECTED_VALUE,
        },
        &executor,
    )
    .await
    .unwrap();
    let state = journal.recovery_state(outcome.action_id()).await.unwrap();
    let dispatch_calls = executor.dispatch_calls();
    let verification_calls = executor.verification_calls();

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!(
        "{}.operation-{}.json",
        path.display(),
        outcome.action_id()
    ));
    let _ = std::fs::remove_file(format!(
        "{}.set-value-payload-{}.json",
        path.display(),
        outcome.action_id()
    ));

    (outcome, state, dispatch_calls, verification_calls)
}

#[tokio::test]
async fn mismatch_never_commits_provider_acknowledgement() {
    let (outcome, state, dispatch_calls, verification_calls) =
        run_case("mismatch", WindowsUiaSetValueEquality::Mismatch).await;

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified { .. }
    ));
    assert_ne!(state, ConsequentialRecoveryState::Committed);
    assert_eq!(dispatch_calls, 1);
    assert_eq!(verification_calls, 1);
}

#[tokio::test]
async fn unknown_never_commits_provider_acknowledgement() {
    let (outcome, state, dispatch_calls, verification_calls) =
        run_case("unknown", WindowsUiaSetValueEquality::Unknown).await;

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified { .. }
    ));
    assert_ne!(state, ConsequentialRecoveryState::Committed);
    assert_eq!(dispatch_calls, 1);
    assert_eq!(verification_calls, 1);
}

#[tokio::test]
async fn match_is_required_for_verified_commit() {
    let (outcome, state, dispatch_calls, verification_calls) =
        run_case("match", WindowsUiaSetValueEquality::Match).await;

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::Committed { .. }
    ));
    assert_eq!(state, ConsequentialRecoveryState::Committed);
    assert_eq!(dispatch_calls, 1);
    assert_eq!(verification_calls, 1);
}

trait OutcomeActionId {
    fn action_id(&self) -> Uuid;
}

impl OutcomeActionId for WindowsUiaVerifiedExecutionOutcome {
    fn action_id(&self) -> Uuid {
        match self {
            WindowsUiaVerifiedExecutionOutcome::Committed { action_id, .. }
            | WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched { action_id, .. }
            | WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified { action_id, .. } => {
                *action_id
            }
        }
    }
}
