use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,
    ConsequentialPostconditionStatus, ConsequentialRecoveryState, LiveBridge,
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
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightRequest,
    WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutor,
    WindowsUiaDispatchSealRequest, WindowsUiaPostconditionVerifier,
    WindowsUiaPreparedDispatchRequest, WindowsUiaProviderExecutionReceipt,
    WindowsUiaProviderExecutionRequest, WindowsUiaVerifiedExecutionOutcome,
    arm_uia_dispatch_execution, execute_armed_uia_dispatch, execute_armed_uia_dispatch_verified,
    prepare_uia_dispatch,
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
            provider: ProviderIncarnationRef::from("provider:windows-uia:coordinator"),
            target: TargetIncarnationRef::from("target:windows:coordinator"),
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

    fn context_calls(&self) -> usize {
        self.state.lock().unwrap().context_calls
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
                opaque_provider_element_id: "uia-runtime:[102,1]".into(),
                semantic_locator_hints: vec!["automation_id=coordinator".into()],
                parent_surface_ref: Some("window:coordinator".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Coordinator".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("coordinator".into()),
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
                surface_scope: "window:coordinator".into(),
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
        self.state.lock().unwrap().context_calls += 1;
        Ok(WindowsUiaBoundDispatchContextReceipt {
            requirements: request.requirements,
            context: WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                element_ref: request.element_ref,
                observation: WindowsUiaDispatchContextObservation {
                    target_window_handle: 0x1020,
                    target_process_id: 102,
                    foreground_window_handle: Some(0x1020),
                    foreground_process_id: Some(102),
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

#[derive(Debug, Clone, Copy)]
enum ExecutorMode {
    Dispatched,
    KnownNotDispatched,
    Fail,
    ForgeElement,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake executor failure")]
struct FakeExecutorError;

#[derive(Debug)]
struct FakeExecutor {
    mode: ExecutorMode,
    calls: Mutex<usize>,
}

impl FakeExecutor {
    fn new(mode: ExecutorMode) -> Self {
        Self {
            mode,
            calls: Mutex::new(0),
        }
    }

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
        if matches!(self.mode, ExecutorMode::Fail) {
            return Err(FakeExecutorError);
        }

        let mut element_ref = request.element_ref().clone();
        if matches!(self.mode, ExecutorMode::ForgeElement) {
            element_ref.opaque_provider_element_id = "uia-runtime:[102,forged]".into();
        }
        let dispatch_result = match self.mode {
            ExecutorMode::Dispatched => DispatchResult::DispatchedFull,
            ExecutorMode::KnownNotDispatched => DispatchResult::DispatchBlockedFocus,
            ExecutorMode::Fail | ExecutorMode::ForgeElement => DispatchResult::DispatchedFull,
        };

        Ok(WindowsUiaProviderExecutionReceipt {
            dispatch_attempt_ref: request.dispatch_attempt_ref(),
            action_id: request.action_id(),
            preparation_journal_sequence: request.preparation_journal_sequence(),
            preparation_receipt_ref: request.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref,
            required_pattern: request.required_pattern(),
            context_requirements: request.context_requirements(),
            transport_result: TransportResult::DeliveredToExecutor,
            dispatch_result,
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum VerifierMode {
    Pass,
    Unknown,
    Fail,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("fake postcondition verifier failure")]
struct FakeVerifierError;

#[derive(Debug)]
struct FakeVerifier {
    mode: VerifierMode,
    calls: Mutex<usize>,
    cuts: Mutex<Vec<String>>,
}

impl FakeVerifier {
    fn new(mode: VerifierMode) -> Self {
        Self {
            mode,
            calls: Mutex::new(0),
            cuts: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }

    fn observed_cuts(&self) -> Vec<String> {
        self.cuts.lock().unwrap().clone()
    }
}

impl WindowsUiaPostconditionVerifier for FakeVerifier {
    type Error = FakeVerifierError;

    fn verify(
        &self,
        _action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        *self.calls.lock().unwrap() += 1;
        self.cuts
            .lock()
            .unwrap()
            .push(snapshot.snapshot_cut_ref().to_owned());
        let status = match self.mode {
            VerifierMode::Pass => ConsequentialPostconditionStatus::VerifiedPass,
            VerifierMode::Unknown => ConsequentialPostconditionStatus::Unknown,
            VerifierMode::Fail => ConsequentialPostconditionStatus::VerifiedFail,
        };
        Ok(expected_contract_refs
            .iter()
            .map(|contract_ref| ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status,
                receipt_ref: format!("verifier:{}:{}", snapshot.snapshot_cut_ref(), contract_ref),
            })
            .collect())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x1021)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x1020,
        expected_process_id: 102,
        selection_nonce: Uuid::from_u128(0x1022),
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
        decision_principal_ref: PrincipalRef::from("principal:decision:coordinator"),
        acting_principal_ref: PrincipalRef::from("principal:acting:coordinator"),
        authorization_revision: "authorization:coordinator:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:coordinator".into()],
    }
}

fn journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-windows-{label}-{}.jsonl",
        Uuid::new_v4()
    ))
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

async fn prepared_and_armed(
    label: &str,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    FakeProvider,
    localview_windows_observe_runtime::WindowsUiaDispatchExecutionPermit,
) {
    let (bridge, journal, path, provider, runtime, seal_request) = fixture(label).await;
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
    let armed = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap();
    (bridge, journal, path, provider, armed)
}

async fn verified_prepared_and_armed(
    label: &str,
) -> (
    LiveBridge,
    ConsequentialJournal,
    PathBuf,
    FakeProvider,
    WindowsObserveRuntimeManager<FakeProvider>,
    localview_windows_observe_runtime::WindowsUiaDispatchExecutionPermit,
) {
    let (bridge, journal, path, provider, runtime, seal_request) = fixture(label).await;
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
    let armed = arm_uia_dispatch_execution(&bridge, &journal, &runtime, session(), prepared)
        .await
        .unwrap();
    (bridge, journal, path, provider, runtime, armed)
}

#[tokio::test]
async fn stale_canonical_authority_before_executor_releases_live_execution_grant() {
    let (bridge, journal, path, _provider, _runtime, armed) =
        verified_prepared_and_armed("stale-canonical-before-executor").await;
    let action_id = armed.action_id();
    assert!(
        bridge.release_provider_observation(session()).await,
        "test must invalidate the provider-bound canonical freshness after arming"
    );
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);

    let error = execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WindowsUiaDispatchExecutionCoordinatorError::CanonicalEnvelopeStaleBeforeExecutor
    ));
    assert_eq!(
        executor.call_count(),
        0,
        "stale canonical authority must fail before provider execution"
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );

    let observation = journal
        .begin_postcondition_observation(action_id)
        .await
        .expect("pre-executor rejection must release live execution authority for reconciliation");
    journal
        .abandon_postcondition_observation(observation)
        .await
        .unwrap();

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn exact_provider_receipt_is_durably_linearized_before_returning_success() {
    let (bridge, journal, path, provider, armed) = prepared_and_armed("coordinator-success").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);

    let result = execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap();

    assert_eq!(executor.call_count(), 1);
    assert_eq!(provider.context_calls(), 2);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::PossiblyDispatched)
    );
    assert!(matches!(
        &result.journal_entry.transition,
        ConsequentialJournalTransition::DispatchLinearized { receipt }
            if receipt.transport_result == TransportResult::DeliveredToExecutor
                && receipt.dispatch_result == DispatchResult::DispatchedFull
                && receipt.receipt_ref
                    == format!("windows-uia:dispatch-attempt:{}", result.provider_receipt.dispatch_attempt_ref)
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn exact_known_not_dispatched_receipt_is_recorded_as_known_not_dispatched() {
    let (bridge, journal, path, _provider, armed) =
        prepared_and_armed("coordinator-not-dispatched").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::KnownNotDispatched);

    execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap();

    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::KnownNotDispatched)
    );
    assert_eq!(
        journal.requires_reconciliation(action_id).await,
        Some(false)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn provider_failure_consumes_execution_authority_and_leaves_prepared_for_reconciliation() {
    let (bridge, journal, path, _provider, armed) =
        prepared_and_armed("coordinator-provider-failure").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Fail);

    let error = execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        WindowsUiaDispatchExecutionCoordinatorError::ProviderExecutionFailed { .. }
    ));
    assert_eq!(executor.call_count(), 1);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    assert_eq!(journal.requires_reconciliation(action_id).await, Some(true));
    let observation = journal
        .begin_postcondition_observation(action_id)
        .await
        .expect("provider failure must release the live execution grant for same-process reconciliation");
    journal
        .abandon_postcondition_observation(observation)
        .await
        .unwrap();

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn forged_provider_receipt_is_never_linearized_and_leaves_prepared_for_reconciliation() {
    let (bridge, journal, path, _provider, armed) =
        prepared_and_armed("coordinator-forged-receipt").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::ForgeElement);

    let error = execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
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
    let observation = journal
        .begin_postcondition_observation(action_id)
        .await
        .expect("forged provider receipt must release the live execution grant for same-process reconciliation");
    journal
        .abandon_postcondition_observation(observation)
        .await
        .unwrap();

    let entries = journal.entries_for(action_id).await;
    assert!(!entries.iter().any(|entry| matches!(
        entry.transition,
        ConsequentialJournalTransition::DispatchLinearized { .. }
    )));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn verified_execution_commits_only_after_fresh_postdispatch_snapshot_passes() {
    let (bridge, journal, path, provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-pass").await;
    let action_id = armed.action_id();
    let pre_dispatch_cut = provider.snapshot().snapshot_cut_ref().to_owned();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Pass);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
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
    assert_eq!(verifier.call_count(), 1);
    let cuts = verifier.observed_cuts();
    assert_eq!(cuts.len(), 1);
    assert_ne!(cuts[0], pre_dispatch_cut);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn unknown_postcondition_never_becomes_committed_success() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-unknown").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Unknown);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            world_outcome: WorldOutcome::ReconciliationRequired,
            ..
        }
    ));
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );
    assert!(
        journal
            .entries_for(action_id)
            .await
            .iter()
            .all(|entry| !matches!(entry.transition, ConsequentialJournalTransition::Committed))
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn verified_failure_never_commits_expected_world_success() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-fail").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);
    let verifier = FakeVerifier::new(VerifierMode::Fail);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
            world_outcome: WorldOutcome::VerifiedUnexpected,
            ..
        }
    ));
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::OutcomeObservedUnverified)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn known_not_dispatched_does_not_invoke_postcondition_verifier() {
    let (bridge, journal, path, _provider, runtime, armed) =
        verified_prepared_and_armed("verified-execution-not-dispatched").await;
    let action_id = armed.action_id();
    let executor = FakeExecutor::new(ExecutorMode::KnownNotDispatched);
    let verifier = FakeVerifier::new(VerifierMode::Pass);

    let outcome = execute_armed_uia_dispatch_verified(
        &bridge,
        &journal,
        &runtime,
        session(),
        armed,
        &executor,
        &verifier,
    )
    .await
    .unwrap();

    assert!(matches!(
        outcome,
        WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            dispatch_result: DispatchResult::DispatchBlockedFocus,
            ..
        }
    ));
    assert_eq!(verifier.call_count(), 0);
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::KnownNotDispatched)
    );

    let _ = std::fs::remove_file(path);
}
