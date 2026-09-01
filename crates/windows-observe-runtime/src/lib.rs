#![forbid(unsafe_code)]

use localview_live_bridge::{
    LiveBridge, ObservationStatus, ObserverEvent, ObserverEventKind, ProviderIngestReport,
    ProviderObservationBinding, ProviderObservationBindingError, ProviderObserverBatch,
};
use localview_native_provider::NativeSemanticSnapshotRevision;
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use localview_windows_uia_provider::{WindowsUiaEvent, WindowsUiaEventDrain, WindowsUiaEventKind};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsObserveBridgeBinding {
    session_id: SessionId,
    generation: u64,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    sequence_baseline: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsObserveBridgeError {
    #[error("provider observation binding rejected: {0:?}")]
    Binding(ProviderObservationBindingError),
    #[error("Windows UIA observation binding is missing")]
    ObservationBindingMissing,
    #[error("LiveBridge provider incarnation does not match the Windows UIA runtime binding")]
    BoundProviderIncarnationMismatch,
    #[error("LiveBridge target incarnation does not match the Windows UIA runtime binding")]
    BoundTargetIncarnationMismatch,
    #[error("LiveBridge generation does not match the Windows UIA runtime binding")]
    BoundGenerationMismatch,
    #[error("Windows UIA event provider incarnation does not match the runtime binding")]
    ProviderIncarnationMismatch,
    #[error("Windows UIA event target incarnation does not match the runtime binding")]
    TargetIncarnationMismatch,
    #[error("Windows UIA event element provider incarnation does not match the runtime binding")]
    ElementProviderIncarnationMismatch,
    #[error("Windows UIA event element target incarnation does not match the runtime binding")]
    ElementTargetIncarnationMismatch,
    #[error("Windows UIA drain contains a non-increasing provider sequence")]
    NonIncreasingSequence,
    #[error("Windows UIA drain latest sequence is behind retained event evidence")]
    InvalidLatestSequence,
    #[error("semantic reconciliation snapshot provider incarnation does not match the runtime binding")]
    ReconciliationProviderIncarnationMismatch,
    #[error("semantic reconciliation snapshot target incarnation does not match the runtime binding")]
    ReconciliationTargetIncarnationMismatch,
    #[error("LiveBridge rejected the reconciliation receipt")]
    ReconciliationRejected,
    #[error("LiveBridge observation state disappeared after reconciliation")]
    ObservationStateMissing,
}

impl WindowsObserveBridgeBinding {
    pub fn new(
        session_id: SessionId,
        generation: u64,
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        sequence_baseline: u64,
    ) -> Self {
        Self {
            session_id,
            generation,
            provider_incarnation_ref,
            target_incarnation_ref,
            sequence_baseline,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn sequence_baseline(&self) -> u64 {
        self.sequence_baseline
    }

    /// Bind the conservative Windows UIA reliability model before accepting
    /// provider events. Opaque ordering is an explicit assurance state, not an
    /// inference from whether the first few callbacks happen to be contiguous.
    pub async fn bind(
        &self,
        bridge: &LiveBridge,
    ) -> Result<ObservationStatus, WindowsObserveBridgeError> {
        bridge
            .bind_provider_observation(ProviderObservationBinding {
                session_id: self.session_id,
                generation: self.generation,
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: self.target_incarnation_ref.clone(),
                initial_continuity: EventContinuityState::OrderingOpaque,
                sequence_baseline: Some(self.sequence_baseline),
            })
            .await
            .map_err(WindowsObserveBridgeError::Binding)
    }

    /// Translate one bounded provider drain into V4.3 LiveBridge evidence.
    ///
    /// Lineage is validated before any bridge mutation. Provider-local UIA
    /// element identity is never promoted into the legacy global `ElementRef`;
    /// authority stays scoped to the provider and target incarnations.
    pub async fn ingest_drain(
        &self,
        bridge: &LiveBridge,
        drain: WindowsUiaEventDrain,
    ) -> Result<ProviderIngestReport, WindowsObserveBridgeError> {
        self.require_live_binding(bridge).await?;
        self.validate_drain(&drain)?;

        let dropped_before_drain = drain.dropped_before_drain;
        let events = drain
            .events
            .into_iter()
            .enumerate()
            .map(|(index, event)| self.project_event(event, index == 0, dropped_before_drain))
            .collect();

        Ok(bridge
            .ingest_provider(ProviderObserverBatch {
                session_id: self.session_id,
                generation: self.generation,
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: self.target_incarnation_ref.clone(),
                events,
            })
            .await)
    }

    /// Record a receipt projected from the exact immutable snapshot revision.
    /// LiveBridge deliberately keeps event continuity debt orthogonal to current
    /// snapshot completeness, so this operation cannot launder a prior gap.
    pub async fn record_snapshot_reconciliation(
        &self,
        bridge: &LiveBridge,
        snapshot: &NativeSemanticSnapshotRevision,
        receipt_id: impl Into<String>,
    ) -> Result<ObservationStatus, WindowsObserveBridgeError> {
        self.require_live_binding(bridge).await?;
        if snapshot.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(WindowsObserveBridgeError::ReconciliationProviderIncarnationMismatch);
        }
        if snapshot.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(WindowsObserveBridgeError::ReconciliationTargetIncarnationMismatch);
        }

        let receipt = snapshot.reconciliation_receipt(receipt_id);
        if !bridge.record_reconciliation(self.session_id, receipt).await {
            return Err(WindowsObserveBridgeError::ReconciliationRejected);
        }
        bridge
            .observation_status(self.session_id)
            .await
            .ok_or(WindowsObserveBridgeError::ObservationStateMissing)
    }

    async fn require_live_binding(
        &self,
        bridge: &LiveBridge,
    ) -> Result<ObservationStatus, WindowsObserveBridgeError> {
        let status = bridge
            .observation_status(self.session_id)
            .await
            .ok_or(WindowsObserveBridgeError::ObservationBindingMissing)?;
        if status.provider_incarnation_ref != self.provider_incarnation_ref {
            return Err(WindowsObserveBridgeError::BoundProviderIncarnationMismatch);
        }
        if status.target_incarnation_ref != self.target_incarnation_ref {
            return Err(WindowsObserveBridgeError::BoundTargetIncarnationMismatch);
        }
        if status.generation != self.generation {
            return Err(WindowsObserveBridgeError::BoundGenerationMismatch);
        }
        Ok(status)
    }

    fn validate_drain(&self, drain: &WindowsUiaEventDrain) -> Result<(), WindowsObserveBridgeError> {
        let mut previous_sequence = None;
        for event in &drain.events {
            if event.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsObserveBridgeError::ProviderIncarnationMismatch);
            }
            if event.target_incarnation_ref != self.target_incarnation_ref {
                return Err(WindowsObserveBridgeError::TargetIncarnationMismatch);
            }
            if let Some(element_ref) = &event.element_ref {
                if element_ref.provider_incarnation_ref != self.provider_incarnation_ref {
                    return Err(WindowsObserveBridgeError::ElementProviderIncarnationMismatch);
                }
                if element_ref.target_incarnation_ref != self.target_incarnation_ref {
                    return Err(WindowsObserveBridgeError::ElementTargetIncarnationMismatch);
                }
            }
            if previous_sequence.is_some_and(|previous| event.sequence <= previous) {
                return Err(WindowsObserveBridgeError::NonIncreasingSequence);
            }
            previous_sequence = Some(event.sequence);
        }

        if previous_sequence.is_some_and(|sequence| drain.latest_sequence < sequence) {
            return Err(WindowsObserveBridgeError::InvalidLatestSequence);
        }
        Ok(())
    }

    fn project_event(
        &self,
        event: WindowsUiaEvent,
        first_retained_event: bool,
        dropped_before_drain: u64,
    ) -> ObserverEvent {
        let dropped = if first_retained_event {
            dropped_before_drain
        } else {
            0
        };
        let provider_element_present = event.element_ref.is_some();
        let (kind, payload) = match event.kind {
            WindowsUiaEventKind::PropertyChanged { property_id } => (
                ObserverEventKind::SemanticSnapshot,
                json!({
                    "native_provider": "windows_uia",
                    "native_event": "property_changed",
                    "property_id": property_id,
                    "dropped_before_drain": dropped,
                    "provider_element_present": provider_element_present,
                    "reliability_profile_revision": "windows-uia-events-v1",
                }),
            ),
            WindowsUiaEventKind::StructureChanged { change_type } => (
                ObserverEventKind::SemanticSnapshot,
                json!({
                    "native_provider": "windows_uia",
                    "native_event": "structure_changed",
                    "change_type": change_type,
                    "dropped_before_drain": dropped,
                    "provider_element_present": provider_element_present,
                    "reliability_profile_revision": "windows-uia-events-v1",
                }),
            ),
            WindowsUiaEventKind::FocusChanged => (
                ObserverEventKind::Focus,
                json!({
                    "native_provider": "windows_uia",
                    "native_event": "focus_changed",
                    "dropped_before_drain": dropped,
                    "provider_element_present": provider_element_present,
                    "reliability_profile_revision": "windows-uia-events-v1",
                }),
            ),
        };

        ObserverEvent {
            seq: event.sequence,
            captured_at: event.captured_at,
            kind,
            reference: None,
            route: None,
            payload,
        }
    }
}
