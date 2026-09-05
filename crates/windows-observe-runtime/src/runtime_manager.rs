use std::{collections::HashMap, error::Error as StdError, fmt, sync::Arc};

use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    LiveBridge, ObservationStatus, ProviderIngestReport,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotRevision, SnapshotResourceUsage,
    UserSelectedWindowTarget,
};
use localview_protocol::{
    EventContinuityState, ProviderElementRef, ProviderIncarnationRef, ReconciliationCompleteness,
    SessionId, TargetIncarnationRef,
};
use localview_resource_governor::{
    PressureLevel, ResourceAdmissionDenial, ResourceReservation, ResourceWorkKind,
    RuntimeResourceGovernor,
};
use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaBoundDispatchContextReceipt, WindowsUiaDispatchContextRequest,
    WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest, WindowsUiaEventDrain,
    WindowsUiaEventSubscription, WindowsUiaEventSubscriptionOptions,
    WindowsUiaPatternDispatchRequest, WindowsUiaSnapshotRequest, WindowsUiaWorker,
    WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
use thiserror::Error;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{WindowsObserveBridgeBinding, WindowsObserveBridgeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsObserveRuntimeConfig {
    pub event_capacity: usize,
    pub drain_limit: usize,
}

impl Default for WindowsObserveRuntimeConfig {
    fn default() -> Self {
        Self {
            event_capacity: 256,
            drain_limit: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsObserveSubscriptionLineage {
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub sequence_baseline: u64,
}

pub trait WindowsObserveProvider: Send + Sync + 'static {
    type Attachment: Clone + Send + Sync + 'static;
    type Subscription: Clone + Send + Sync + 'static;
    type Error: StdError + Send + Sync + 'static;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef;

    fn attach(&self, selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error>;

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef;

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        capacity: usize,
    ) -> Result<Self::Subscription, Self::Error>;

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage;

    fn drain_events(
        &self,
        subscription: &Self::Subscription,
        limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error>;

    fn snapshot(
        &self,
        attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error>;

    fn unsubscribe_events(&self, subscription: Self::Subscription) -> Result<(), Self::Error>;
}

/// Narrow capability used only by the Phase 6 dispatch eligibility fence.
///
/// Observe-only providers do not need to implement this trait. Implementations
/// may bind a worker-owned live element but must return only a data receipt and
/// must not execute a UIA pattern method or OS input as part of the bind.
pub trait WindowsObserveActionLeaseProvider: WindowsObserveProvider {
    fn bind_element_lease(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, Self::Error>;
}

/// Narrow capability for the last volatile provider-context fence.
///
/// Implementations must observe context against the already-retained exact live
/// element and return only data. No UIA write pattern or OS input may be emitted
/// while collecting this receipt.
pub trait WindowsObserveDispatchContextProvider: WindowsObserveActionLeaseProvider {
    fn revalidate_dispatch_context(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaBoundDispatchContextReceipt, Self::Error>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsObserveRuntimeError {
    #[error("Windows observe runtime configuration is invalid")]
    InvalidConfiguration,
    #[error("Windows observe session {session_id} is already attached")]
    AlreadyAttached { session_id: SessionId },
    #[error("Windows observe session {session_id} is not attached")]
    NotAttached { session_id: SessionId },
    #[error("Windows observe provider failed during {operation}: {message}")]
    Provider {
        operation: &'static str,
        message: String,
    },
    #[error("Windows observe provider task failed during {operation}: {message}")]
    ProviderTask {
        operation: &'static str,
        message: String,
    },
    #[error("Windows observe resource governor denied {work_kind:?} at {pressure:?} pressure")]
    ResourceDenied {
        work_kind: ResourceWorkKind,
        pressure: PressureLevel,
        reasons: Vec<String>,
    },
    #[error("Windows observe subscription provider lineage does not match the worker")]
    SubscriptionProviderIncarnationMismatch,
    #[error("Windows observe subscription target lineage does not match the attachment")]
    SubscriptionTargetIncarnationMismatch,
    #[error("Windows observe bridge operation failed: {0}")]
    Bridge(WindowsObserveBridgeError),
    #[error("Windows observe LiveBridge state disappeared for session {session_id}")]
    ObservationStateMissing { session_id: SessionId },
}

impl From<WindowsObserveBridgeError> for WindowsObserveRuntimeError {
    fn from(value: WindowsObserveBridgeError) -> Self {
        Self::Bridge(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSemanticReadRequest {
    pub authority: ActionEnvelopeMetadata,
    pub element_ref: ProviderElementRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsSemanticReadReceipt {
    pub snapshot_cut_ref: String,
    pub cache_revision_ref: String,
    pub observed_digest: String,
    pub node: NativeSemanticNodeObservation,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsSemanticReadError {
    #[error("Windows semantic read session {session_id} is not attached")]
    NotAttached { session_id: SessionId },
    #[error("Windows semantic read canonical authority rejected: {0:?}")]
    Authority(ActionEnvelopeBindingError),
    #[error("Windows semantic read requires S0 observe-only risk")]
    ObserveOnlyRiskRequired,
    #[error("Windows semantic read requires pure-read idempotency")]
    PureReadIdempotencyRequired,
    #[error("Windows semantic read precondition cut does not match the current snapshot")]
    PreconditionSnapshotCutMismatch { expected: String, actual: String },
    #[error("Windows semantic read current snapshot is incomplete")]
    SnapshotIncomplete,
    #[error("Windows semantic read element acquisition cut does not match the current snapshot")]
    ElementAcquisitionCutMismatch { expected: String, actual: String },
    #[error("Windows semantic read element does not exist in the exact current snapshot")]
    ElementNotFound,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsObserveResourceAccounting {
    pub initial_snapshots: u64,
    pub reconciliation_snapshots: u64,
    pub event_drains: u64,
    pub events_accepted: u64,
    pub events_rejected_stale: u64,
    pub provider_events_dropped: u64,
    pub snapshot_nodes_observed: u64,
    pub snapshot_properties_read: u64,
    pub snapshot_max_depth_observed: usize,
    pub incomplete_snapshots: u64,
    pub resource_denials: u64,
}

impl WindowsObserveResourceAccounting {
    fn record_initial_snapshot(&mut self, usage: &SnapshotResourceUsage) {
        self.initial_snapshots = self.initial_snapshots.saturating_add(1);
        self.record_snapshot_usage(usage);
    }

    fn record_reconciliation_snapshot(&mut self, usage: &SnapshotResourceUsage) {
        self.reconciliation_snapshots = self.reconciliation_snapshots.saturating_add(1);
        self.record_snapshot_usage(usage);
    }

    fn record_snapshot_usage(&mut self, usage: &SnapshotResourceUsage) {
        self.snapshot_nodes_observed = self
            .snapshot_nodes_observed
            .saturating_add(usize_to_u64(usage.nodes_observed));
        self.snapshot_properties_read = self
            .snapshot_properties_read
            .saturating_add(usize_to_u64(usage.properties_read));
        self.snapshot_max_depth_observed = self
            .snapshot_max_depth_observed
            .max(usage.max_depth_observed);
        if usage.incomplete {
            self.incomplete_snapshots = self.incomplete_snapshots.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsObserveDrainOutcome {
    pub report: ProviderIngestReport,
    pub status: ObservationStatus,
    pub reconciliation_performed: bool,
}

struct ActiveObservation<P: WindowsObserveProvider> {
    attachment: P::Attachment,
    subscription: P::Subscription,
    binding: WindowsObserveBridgeBinding,
    surface_scope: String,
    current_snapshot: Arc<NativeSemanticSnapshotRevision>,
    accounting: WindowsObserveResourceAccounting,
}

pub struct WindowsObserveRuntimeManager<P: WindowsObserveProvider> {
    provider: Arc<P>,
    bridge: LiveBridge,
    config: WindowsObserveRuntimeConfig,
    resource_governor: RuntimeResourceGovernor,
    active: Arc<Mutex<HashMap<SessionId, ActiveObservation<P>>>>,
    generations: Mutex<HashMap<SessionId, u64>>,
    operation_gate: Arc<Mutex<()>>,
}

impl<P: WindowsObserveProvider> fmt::Debug for WindowsObserveRuntimeManager<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsObserveRuntimeManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

pub struct WindowsUiaRuntimeDispatchExecutor {
    provider: Arc<WindowsUiaObserveProvider>,
    active: Arc<Mutex<HashMap<SessionId, ActiveObservation<WindowsUiaObserveProvider>>>>,
    operation_gate: Arc<Mutex<()>>,
    session_id: SessionId,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
}

impl fmt::Debug for WindowsUiaRuntimeDispatchExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsUiaRuntimeDispatchExecutor")
            .field("session_id", &self.session_id)
            .field("provider_incarnation_ref", &self.provider_incarnation_ref)
            .field("target_incarnation_ref", &self.target_incarnation_ref)
            .finish_non_exhaustive()
    }
}

impl crate::WindowsUiaDispatchExecutor for WindowsUiaRuntimeDispatchExecutor {
    type Error = WindowsObserveRuntimeError;

    async fn execute(
        &self,
        request: &crate::WindowsUiaProviderExecutionRequest,
    ) -> Result<crate::WindowsUiaProviderExecutionReceipt, Self::Error> {
        let provider_request = WindowsUiaPatternDispatchRequest {
            dispatch_attempt_ref: request.dispatch_attempt_ref(),
            action_id: request.action_id(),
            preparation_journal_sequence: request.preparation_journal_sequence(),
            preparation_receipt_ref: request.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref: request.element_ref().clone(),
            required_pattern: request.required_pattern(),
            context_requirements: request.context_requirements(),
        };

        // Serialize the exact side-effect window against attach/drain/reconcile/release.
        // Re-resolve the active attachment under this gate so an executor resolved
        // earlier cannot dispatch after its session was detached or reincarnated.
        let _gate = self.operation_gate.lock().await;
        let attachment = self
            .active
            .lock()
            .await
            .get(&self.session_id)
            .map(|observation| observation.attachment.clone())
            .ok_or(WindowsObserveRuntimeError::NotAttached {
                session_id: self.session_id,
            })?;
        if attachment.provider_incarnation_ref() != &self.provider_incarnation_ref
            || attachment.target_incarnation_ref() != &self.target_incarnation_ref
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "dispatch_pattern_session_revalidation",
                message: "attached Windows UIA session lineage changed after executor resolution"
                    .into(),
            });
        }

        let provider = self.provider.clone();
        let dispatch_attachment = attachment.clone();
        let receipt = run_provider("dispatch_pattern", move || {
            provider
                .worker
                .dispatch_pattern(&dispatch_attachment, provider_request)
        })
        .await?;

        Ok(crate::WindowsUiaProviderExecutionReceipt {
            dispatch_attempt_ref: receipt.dispatch_attempt_ref,
            action_id: receipt.action_id,
            preparation_journal_sequence: receipt.preparation_journal_sequence,
            preparation_receipt_ref: receipt.preparation_receipt_ref,
            snapshot_cut_ref: receipt.snapshot_cut_ref,
            provider_incarnation_ref: receipt.provider_incarnation_ref,
            target_incarnation_ref: receipt.target_incarnation_ref,
            element_ref: receipt.element_ref,
            required_pattern: receipt.required_pattern,
            context_requirements: receipt.context_requirements,
            transport_result: receipt.transport_result,
            dispatch_result: receipt.dispatch_result,
        })
    }
}

impl<P: WindowsObserveProvider> WindowsObserveRuntimeManager<P> {
    pub fn new(
        provider: Arc<P>,
        bridge: LiveBridge,
        config: WindowsObserveRuntimeConfig,
    ) -> Result<Self, WindowsObserveRuntimeError> {
        Self::with_resource_governor(provider, bridge, config, RuntimeResourceGovernor::default())
    }

    pub fn with_resource_governor(
        provider: Arc<P>,
        bridge: LiveBridge,
        config: WindowsObserveRuntimeConfig,
        resource_governor: RuntimeResourceGovernor,
    ) -> Result<Self, WindowsObserveRuntimeError> {
        if config.event_capacity == 0 || config.drain_limit == 0 {
            return Err(WindowsObserveRuntimeError::InvalidConfiguration);
        }

        Ok(Self {
            provider,
            bridge,
            config,
            resource_governor,
            active: Arc::new(Mutex::new(HashMap::new())),
            generations: Mutex::new(HashMap::new()),
            operation_gate: Arc::new(Mutex::new(())),
        })
    }

    pub async fn attach(
        &self,
        session_id: SessionId,
        selection: UserSelectedWindowTarget,
    ) -> Result<ObservationStatus, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        if self.active.lock().await.contains_key(&session_id) {
            return Err(WindowsObserveRuntimeError::AlreadyAttached { session_id });
        }

        // Admission precedes every provider call. Critical runtime pressure must
        // therefore fail before attach/subscribe/snapshot work begins.
        let _observation_reservation = self
            .reserve_resource(session_id, ResourceWorkKind::NativeSemanticObservation)
            .await?;

        let provider = self.provider.clone();
        let attach_selection = selection.clone();
        let attachment = run_provider("attach", move || provider.attach(attach_selection)).await?;

        let provider_incarnation_ref = self.provider.provider_incarnation_ref();
        let target_incarnation_ref = self.provider.target_incarnation_ref(&attachment);
        let provider = self.provider.clone();
        let subscribe_attachment = attachment.clone();
        let event_capacity = self.config.event_capacity;
        let subscription = run_provider("subscribe_events", move || {
            provider.subscribe_events(&subscribe_attachment, event_capacity)
        })
        .await?;
        let lineage = self.provider.subscription_lineage(&subscription);

        if lineage.provider_incarnation_ref != provider_incarnation_ref {
            self.best_effort_unsubscribe(subscription).await;
            return Err(WindowsObserveRuntimeError::SubscriptionProviderIncarnationMismatch);
        }
        if lineage.target_incarnation_ref != target_incarnation_ref {
            self.best_effort_unsubscribe(subscription).await;
            return Err(WindowsObserveRuntimeError::SubscriptionTargetIncarnationMismatch);
        }

        let generation = self.next_generation(session_id).await;
        let surface_scope = format!("window:hwnd={:x}", selection.native_window_handle);
        let snapshot_cut_ref = format!(
            "windows-uia:initial:{session_id}:{generation}:{}",
            Uuid::new_v4()
        );
        let provider = self.provider.clone();
        let snapshot_attachment = attachment.clone();
        let snapshot_surface = surface_scope.clone();
        let snapshot = match run_provider("initial_snapshot", move || {
            provider.snapshot(&snapshot_attachment, snapshot_cut_ref, snapshot_surface)
        })
        .await
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.best_effort_unsubscribe(subscription).await;
                return Err(error);
            }
        };

        let binding = WindowsObserveBridgeBinding::new(
            session_id,
            generation,
            provider_incarnation_ref,
            target_incarnation_ref,
            lineage.sequence_baseline,
        );
        if let Err(error) = binding.bind(&self.bridge).await {
            self.best_effort_unsubscribe(subscription).await;
            return Err(error.into());
        }

        let receipt_id = format!(
            "reconcile:windows-uia:initial:{session_id}:{generation}:{}",
            Uuid::new_v4()
        );
        let status = match binding
            .record_snapshot_reconciliation(&self.bridge, snapshot.as_ref(), receipt_id)
            .await
        {
            Ok(status) => status,
            Err(error) => {
                self.bridge.release_provider_observation(session_id).await;
                self.best_effort_unsubscribe(subscription).await;
                return Err(error.into());
            }
        };

        let mut accounting = WindowsObserveResourceAccounting::default();
        accounting.record_initial_snapshot(snapshot.resource_usage());
        self.generations.lock().await.insert(session_id, generation);
        self.active.lock().await.insert(
            session_id,
            ActiveObservation {
                attachment,
                subscription,
                binding,
                surface_scope,
                current_snapshot: snapshot,
                accounting,
            },
        );
        Ok(status)
    }

    pub async fn drain_once(
        &self,
        session_id: SessionId,
    ) -> Result<WindowsObserveDrainOutcome, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let (attachment, subscription, binding, surface_scope) = {
            let active = self.active.lock().await;
            let observation = active
                .get(&session_id)
                .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;
            (
                observation.attachment.clone(),
                observation.subscription.clone(),
                observation.binding.clone(),
                observation.surface_scope.clone(),
            )
        };

        let observation_reservation = self
            .reserve_resource(session_id, ResourceWorkKind::NativeSemanticObservation)
            .await?;
        let provider = self.provider.clone();
        let drain_subscription = subscription.clone();
        let drain_limit = self.config.drain_limit;
        let drain = run_provider("drain_events", move || {
            provider.drain_events(&drain_subscription, drain_limit)
        })
        .await?;
        let dropped_before_drain = drain.dropped_before_drain;

        // The bounded provider drain itself happened even if a later bridge
        // validation fails, so count that work before projecting it downstream.
        self.update_accounting(session_id, |accounting| {
            accounting.event_drains = accounting.event_drains.saturating_add(1);
            accounting.provider_events_dropped = accounting
                .provider_events_dropped
                .saturating_add(dropped_before_drain);
        })
        .await;

        let report = binding.ingest_drain(&self.bridge, drain).await?;
        self.update_accounting(session_id, |accounting| {
            accounting.events_accepted = accounting
                .events_accepted
                .saturating_add(usize_to_u64(report.ingest.accepted));
            accounting.events_rejected_stale = accounting
                .events_rejected_stale
                .saturating_add(usize_to_u64(report.ingest.rejected_stale));
        })
        .await;

        let observed_status = self
            .bridge
            .observation_status(session_id)
            .await
            .ok_or(WindowsObserveRuntimeError::ObservationStateMissing { session_id })?;
        let reconciliation_needed = requires_reconciliation(report.continuity)
            && observed_status.current_snapshot_completeness.is_none();

        // Reconciliation is a distinct authority decision. The observation may
        // have been admitted before pressure rose while processing the callback
        // drain; releasing it here forces a fresh governor decision for the
        // correctness-restoring snapshot.
        drop(observation_reservation);

        let status = if reconciliation_needed {
            let _reconciliation_reservation = self
                .reserve_resource(session_id, ResourceWorkKind::NativeSemanticReconciliation)
                .await?;
            let snapshot_cut_ref = format!(
                "windows-uia:reconcile:{session_id}:{}:{}",
                binding.generation(),
                Uuid::new_v4()
            );
            let provider = self.provider.clone();
            let reconcile_attachment = attachment.clone();
            let reconcile_surface = surface_scope.clone();
            let snapshot = run_provider("reconciliation_snapshot", move || {
                provider.snapshot(&reconcile_attachment, snapshot_cut_ref, reconcile_surface)
            })
            .await?;
            let receipt_id = format!(
                "reconcile:windows-uia:gap:{session_id}:{}:{}",
                binding.generation(),
                Uuid::new_v4()
            );
            let status = binding
                .record_snapshot_reconciliation(&self.bridge, snapshot.as_ref(), receipt_id)
                .await?;
            self.update_reconciliation_snapshot(session_id, snapshot)
                .await;
            status
        } else {
            observed_status
        };

        Ok(WindowsObserveDrainOutcome {
            report,
            status,
            reconciliation_performed: reconciliation_needed,
        })
    }

    pub async fn read_semantic(
        &self,
        session_id: SessionId,
        request: WindowsSemanticReadRequest,
    ) -> Result<WindowsSemanticReadReceipt, WindowsSemanticReadError> {
        // Serialize pure reads with attach/drain/reconciliation/release so the
        // snapshot pointer and the bridge-visible reconciliation boundary cannot
        // momentarily describe different current cuts.
        let _gate = self.operation_gate.lock().await;
        let snapshot = self
            .active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.current_snapshot.clone())
            .ok_or(WindowsSemanticReadError::NotAttached { session_id })?;

        validate_semantic_read_authority(&request.authority, snapshot.as_ref())?;
        if snapshot.completeness() != ReconciliationCompleteness::Established {
            return Err(WindowsSemanticReadError::SnapshotIncomplete);
        }

        let current_cut = snapshot.snapshot_cut_ref();
        if request.element_ref.acquisition_cut_ref != current_cut {
            return Err(WindowsSemanticReadError::ElementAcquisitionCutMismatch {
                expected: current_cut.to_owned(),
                actual: request.element_ref.acquisition_cut_ref.clone(),
            });
        }

        let node = snapshot
            .nodes()
            .iter()
            .find(|node| node.element_ref == request.element_ref)
            .cloned()
            .ok_or(WindowsSemanticReadError::ElementNotFound)?;

        Ok(WindowsSemanticReadReceipt {
            snapshot_cut_ref: current_cut.to_owned(),
            cache_revision_ref: snapshot.cache_revision_ref().to_owned(),
            observed_digest: snapshot.observed_digest().to_owned(),
            node,
        })
    }

    pub async fn status(&self, session_id: SessionId) -> Option<ObservationStatus> {
        if !self.active.lock().await.contains_key(&session_id) {
            return None;
        }
        self.bridge.observation_status(session_id).await
    }

    pub async fn resource_accounting(
        &self,
        session_id: SessionId,
    ) -> Option<WindowsObserveResourceAccounting> {
        self.active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.accounting.clone())
    }

    /// Detach observation authority first, then attempt provider-side cleanup.
    ///
    /// A hung or failed provider must never preserve stale LocalView authority.
    /// Provider cleanup errors are returned to the caller, but the manager and
    /// LiveBridge remain detached even when unregistering the OS event handler
    /// fails.
    pub async fn release(&self, session_id: SessionId) -> Result<(), WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let observation = self
            .active
            .lock()
            .await
            .remove(&session_id)
            .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;
        self.bridge.release_provider_observation(session_id).await;

        let provider = self.provider.clone();
        run_provider("unsubscribe_events", move || {
            provider.unsubscribe_events(observation.subscription)
        })
        .await
    }

    pub async fn attached_sessions(&self) -> Vec<SessionId> {
        let mut sessions = self.active.lock().await.keys().copied().collect::<Vec<_>>();
        sessions.sort_unstable();
        sessions
    }

    async fn reserve_resource(
        &self,
        session_id: SessionId,
        work_kind: ResourceWorkKind,
    ) -> Result<ResourceReservation, WindowsObserveRuntimeError> {
        match self.resource_governor.reserve(
            session_id.to_string(),
            format!("windows-observe:{work_kind:?}:{}", Uuid::new_v4()),
            work_kind,
        ) {
            Ok(reservation) => Ok(reservation),
            Err(denial) => {
                self.update_accounting(session_id, |accounting| {
                    accounting.resource_denials = accounting.resource_denials.saturating_add(1);
                })
                .await;
                Err(resource_denial_error(denial))
            }
        }
    }

    async fn update_accounting(
        &self,
        session_id: SessionId,
        update: impl FnOnce(&mut WindowsObserveResourceAccounting),
    ) {
        if let Some(observation) = self.active.lock().await.get_mut(&session_id) {
            update(&mut observation.accounting);
        }
    }

    async fn update_reconciliation_snapshot(
        &self,
        session_id: SessionId,
        snapshot: Arc<NativeSemanticSnapshotRevision>,
    ) {
        if let Some(observation) = self.active.lock().await.get_mut(&session_id) {
            observation
                .accounting
                .record_reconciliation_snapshot(snapshot.resource_usage());
            observation.current_snapshot = snapshot;
        }
    }

    async fn next_generation(&self, session_id: SessionId) -> u64 {
        self.generations
            .lock()
            .await
            .get(&session_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(1)
    }

    async fn best_effort_unsubscribe(&self, subscription: P::Subscription) {
        let provider = self.provider.clone();
        let _ = run_provider("cleanup_unsubscribe_events", move || {
            provider.unsubscribe_events(subscription)
        })
        .await;
    }
}

impl<P> WindowsObserveRuntimeManager<P>
where
    P: WindowsObserveActionLeaseProvider,
{
    /// Bind the exact retained worker element for one current snapshot cut.
    ///
    /// This is intentionally separate from semantic preflight. Re-acquiring the
    /// operation gate makes attach/drain/reconciliation/release mutually
    /// exclusive with the live bind; if the snapshot changed in the gap before
    /// this call, the provider's exact-cut lease contract rejects the request.
    pub(crate) async fn bind_action_element_lease(
        &self,
        session_id: SessionId,
        request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let attachment = self
            .active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.attachment.clone())
            .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;

        let provider = self.provider.clone();
        run_provider("bind_element_lease", move || {
            provider.bind_element_lease(&attachment, request)
        })
        .await
    }
}

impl<P> WindowsObserveRuntimeManager<P>
where
    P: WindowsObserveDispatchContextProvider,
{
    /// Observe the volatile dispatch context against the exact current
    /// attachment while serializing against attach/drain/reconcile/release.
    ///
    /// The provider owns the exact retained UIA element and performs the
    /// foreground/focus/modal read on its MTA. Only a bound data receipt crosses
    /// back into the runtime; this method never performs a side effect.
    pub(crate) async fn revalidate_action_dispatch_context(
        &self,
        session_id: SessionId,
        request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaBoundDispatchContextReceipt, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let attachment = self
            .active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.attachment.clone())
            .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;

        let provider = self.provider.clone();
        run_provider("revalidate_dispatch_context", move || {
            provider.revalidate_dispatch_context(&attachment, request)
        })
        .await
    }
}

fn validate_semantic_read_authority(
    authority: &ActionEnvelopeMetadata,
    snapshot: &NativeSemanticSnapshotRevision,
) -> Result<(), WindowsSemanticReadError> {
    if authority.risk_class != ActionRiskClass::ObserveOnly {
        return Err(WindowsSemanticReadError::ObserveOnlyRiskRequired);
    }
    if authority.idempotency_class != ActionIdempotencyClass::PureRead {
        return Err(WindowsSemanticReadError::PureReadIdempotencyRequired);
    }
    if authority.provider_incarnation_ref != *snapshot.provider_incarnation_ref() {
        return Err(WindowsSemanticReadError::Authority(
            ActionEnvelopeBindingError::ProviderIncarnationMismatch,
        ));
    }
    if authority.target_incarnation_ref != *snapshot.target_incarnation_ref() {
        return Err(WindowsSemanticReadError::Authority(
            ActionEnvelopeBindingError::TargetIncarnationMismatch,
        ));
    }

    let expected_cut = snapshot.snapshot_cut_ref();
    if authority.precondition_snapshot_cut_ref != expected_cut {
        return Err(WindowsSemanticReadError::PreconditionSnapshotCutMismatch {
            expected: expected_cut.to_owned(),
            actual: authority.precondition_snapshot_cut_ref.clone(),
        });
    }
    Ok(())
}

fn resource_denial_error(denial: ResourceAdmissionDenial) -> WindowsObserveRuntimeError {
    WindowsObserveRuntimeError::ResourceDenied {
        work_kind: denial.work_kind,
        pressure: denial.decision.pressure,
        reasons: denial.decision.reasons,
    }
}

fn requires_reconciliation(continuity: EventContinuityState) -> bool {
    matches!(
        continuity,
        EventContinuityState::GapDetected
            | EventContinuityState::SequenceReset
            | EventContinuityState::ProviderReincarnated
            | EventContinuityState::ReconciliationRequired
            | EventContinuityState::ReconnectedUnreconciled
            | EventContinuityState::Broken
    )
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

async fn run_provider<T, E, F>(
    operation: &'static str,
    function: F,
) -> Result<T, WindowsObserveRuntimeError>
where
    T: Send + 'static,
    E: StdError + Send + Sync + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    tokio::task::spawn_blocking(function)
        .await
        .map_err(|error| WindowsObserveRuntimeError::ProviderTask {
            operation,
            message: error.to_string(),
        })?
        .map_err(|error| WindowsObserveRuntimeError::Provider {
            operation,
            message: error.to_string(),
        })
}

pub struct WindowsUiaObserveProvider {
    worker: WindowsUiaWorker,
}

impl fmt::Debug for WindowsUiaObserveProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsUiaObserveProvider")
            .field(
                "provider_incarnation_ref",
                self.worker.provider_incarnation_ref(),
            )
            .finish_non_exhaustive()
    }
}

impl WindowsUiaObserveProvider {
    pub fn spawn(config: WindowsUiaWorkerConfig) -> Result<Self, WindowsUiaWorkerError> {
        Ok(Self {
            worker: WindowsUiaWorker::spawn(config)?,
        })
    }
}

impl WindowsObserveProvider for WindowsUiaObserveProvider {
    type Attachment = WindowsUiaAttachment;
    type Subscription = WindowsUiaEventSubscription;
    type Error = WindowsUiaWorkerError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.worker.provider_incarnation_ref().clone()
    }

    fn attach(&self, selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error> {
        self.worker.attach(selection)
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.target_incarnation_ref().clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        self.worker
            .subscribe_events(attachment, WindowsUiaEventSubscriptionOptions { capacity })
    }

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage {
        WindowsObserveSubscriptionLineage {
            provider_incarnation_ref: subscription.provider_incarnation_ref().clone(),
            target_incarnation_ref: subscription.target_incarnation_ref().clone(),
            sequence_baseline: subscription.sequence_baseline(),
        }
    }

    fn drain_events(
        &self,
        subscription: &Self::Subscription,
        limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error> {
        self.worker.drain_events(subscription, limit)
    }

    fn snapshot(
        &self,
        attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        self.worker.snapshot(
            attachment,
            WindowsUiaSnapshotRequest {
                snapshot_cut_ref,
                surface_scope,
            },
        )
    }

    fn unsubscribe_events(&self, subscription: Self::Subscription) -> Result<(), Self::Error> {
        self.worker.unsubscribe_events(subscription)
    }
}

impl WindowsObserveActionLeaseProvider for WindowsUiaObserveProvider {
    fn bind_element_lease(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, Self::Error> {
        self.worker.bind_element_lease(attachment, request)
    }
}

impl WindowsObserveDispatchContextProvider for WindowsUiaObserveProvider {
    fn revalidate_dispatch_context(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaBoundDispatchContextReceipt, Self::Error> {
        self.worker.revalidate_dispatch_context(attachment, request)
    }
}

impl WindowsObserveRuntimeManager<WindowsUiaObserveProvider> {
    /// Resolve a narrow executor for one exact currently attached Windows UIA session.
    ///
    /// The returned executor never mints authority. At execution time it reacquires
    /// the runtime operation gate, confirms the same provider/target incarnation
    /// remains attached, and only then forwards the coordinator-minted opaque
    /// request to the retained MTA worker.
    pub async fn uia_dispatch_executor(
        &self,
        session_id: SessionId,
    ) -> Result<WindowsUiaRuntimeDispatchExecutor, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let attachment = self
            .active
            .lock()
            .await
            .get(&session_id)
            .map(|observation| observation.attachment.clone())
            .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;

        Ok(WindowsUiaRuntimeDispatchExecutor {
            provider: self.provider.clone(),
            active: self.active.clone(),
            operation_gate: self.operation_gate.clone(),
            session_id,
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
        })
    }
}

pub type WindowsUiaObserveRuntimeManager = WindowsObserveRuntimeManager<WindowsUiaObserveProvider>;

pub fn spawn_windows_uia_runtime_manager(
    bridge: LiveBridge,
    worker_config: WindowsUiaWorkerConfig,
    runtime_config: WindowsObserveRuntimeConfig,
) -> Result<WindowsUiaObserveRuntimeManager, WindowsObserveRuntimeError> {
    spawn_windows_uia_runtime_manager_with_governor(
        bridge,
        RuntimeResourceGovernor::default(),
        worker_config,
        runtime_config,
    )
}

pub fn spawn_windows_uia_runtime_manager_with_governor(
    bridge: LiveBridge,
    resource_governor: RuntimeResourceGovernor,
    worker_config: WindowsUiaWorkerConfig,
    runtime_config: WindowsObserveRuntimeConfig,
) -> Result<WindowsUiaObserveRuntimeManager, WindowsObserveRuntimeError> {
    let provider = WindowsUiaObserveProvider::spawn(worker_config).map_err(|error| {
        WindowsObserveRuntimeError::Provider {
            operation: "spawn",
            message: error.to_string(),
        }
    })?;
    WindowsObserveRuntimeManager::with_resource_governor(
        Arc::new(provider),
        bridge,
        runtime_config,
        resource_governor,
    )
}
