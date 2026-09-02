use std::{
    collections::HashMap,
    error::Error as StdError,
    fmt,
    sync::Arc,
};

use localview_live_bridge::{LiveBridge, ObservationStatus, ProviderIngestReport};
use localview_native_provider::{NativeSemanticSnapshotRevision, UserSelectedWindowTarget};
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaEventDrain, WindowsUiaEventSubscription,
    WindowsUiaEventSubscriptionOptions, WindowsUiaSnapshotRequest, WindowsUiaWorker,
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

    fn attach(
        &self,
        selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error>;

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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
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
}

pub struct WindowsObserveRuntimeManager<P: WindowsObserveProvider> {
    provider: Arc<P>,
    bridge: LiveBridge,
    config: WindowsObserveRuntimeConfig,
    active: Mutex<HashMap<SessionId, ActiveObservation<P>>>,
    generations: Mutex<HashMap<SessionId, u64>>,
    operation_gate: Mutex<()>,
}

impl<P: WindowsObserveProvider> fmt::Debug for WindowsObserveRuntimeManager<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsObserveRuntimeManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<P: WindowsObserveProvider> WindowsObserveRuntimeManager<P> {
    pub fn new(
        provider: Arc<P>,
        bridge: LiveBridge,
        config: WindowsObserveRuntimeConfig,
    ) -> Result<Self, WindowsObserveRuntimeError> {
        if config.event_capacity == 0 || config.drain_limit == 0 {
            return Err(WindowsObserveRuntimeError::InvalidConfiguration);
        }

        Ok(Self {
            provider,
            bridge,
            config,
            active: Mutex::new(HashMap::new()),
            generations: Mutex::new(HashMap::new()),
            operation_gate: Mutex::new(()),
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

        self.generations.lock().await.insert(session_id, generation);
        self.active.lock().await.insert(
            session_id,
            ActiveObservation {
                attachment,
                subscription,
                binding,
                surface_scope,
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

        let provider = self.provider.clone();
        let drain_subscription = subscription.clone();
        let drain_limit = self.config.drain_limit;
        let drain = run_provider("drain_events", move || {
            provider.drain_events(&drain_subscription, drain_limit)
        })
        .await?;

        let report = binding.ingest_drain(&self.bridge, drain).await?;
        let reconciliation_performed = requires_reconciliation(report.continuity);
        let status = if reconciliation_performed {
            let snapshot_cut_ref = format!(
                "windows-uia:reconcile:{session_id}:{}:{}",
                binding.generation(),
                Uuid::new_v4()
            );
            let provider = self.provider.clone();
            let reconcile_attachment = attachment.clone();
            let reconcile_surface = surface_scope.clone();
            let snapshot = run_provider("reconciliation_snapshot", move || {
                provider.snapshot(
                    &reconcile_attachment,
                    snapshot_cut_ref,
                    reconcile_surface,
                )
            })
            .await?;
            let receipt_id = format!(
                "reconcile:windows-uia:gap:{session_id}:{}:{}",
                binding.generation(),
                Uuid::new_v4()
            );
            binding
                .record_snapshot_reconciliation(&self.bridge, snapshot.as_ref(), receipt_id)
                .await?
        } else {
            self.bridge
                .observation_status(session_id)
                .await
                .ok_or(WindowsObserveRuntimeError::ObservationStateMissing { session_id })?
        };

        Ok(WindowsObserveDrainOutcome {
            report,
            status,
            reconciliation_performed,
        })
    }

    pub async fn status(&self, session_id: SessionId) -> Option<ObservationStatus> {
        if !self.active.lock().await.contains_key(&session_id) {
            return None;
        }
        self.bridge.observation_status(session_id).await
    }

    pub async fn release(
        &self,
        session_id: SessionId,
    ) -> Result<(), WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let subscription = {
            let active = self.active.lock().await;
            active
                .get(&session_id)
                .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?
                .subscription
                .clone()
        };

        let provider = self.provider.clone();
        run_provider("unsubscribe_events", move || {
            provider.unsubscribe_events(subscription)
        })
        .await?;

        self.active.lock().await.remove(&session_id);
        self.bridge.release_provider_observation(session_id).await;
        Ok(())
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
            .field("provider_incarnation_ref", self.worker.provider_incarnation_ref())
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

    fn attach(
        &self,
        selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error> {
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

pub type WindowsUiaObserveRuntimeManager = WindowsObserveRuntimeManager<WindowsUiaObserveProvider>;

pub fn spawn_windows_uia_runtime_manager(
    bridge: LiveBridge,
    worker_config: WindowsUiaWorkerConfig,
    runtime_config: WindowsObserveRuntimeConfig,
) -> Result<WindowsUiaObserveRuntimeManager, WindowsObserveRuntimeError> {
    let provider = WindowsUiaObserveProvider::spawn(worker_config).map_err(|error| {
        WindowsObserveRuntimeError::Provider {
            operation: "spawn",
            message: error.to_string(),
        }
    })?;
    WindowsObserveRuntimeManager::new(Arc::new(provider), bridge, runtime_config)
}
