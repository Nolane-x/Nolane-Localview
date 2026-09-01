#![forbid(unsafe_code)]

#[path = "cancellable_lib.rs"]
mod legacy;
mod action_envelope;

pub use action_envelope::*;
pub use legacy::{
    ActionCancellationOutcome, ActionCancellationSignal, ActionCancellationState, BridgeAction,
    BridgeActionKind, BridgeActionResult, CompletionOrigin, IngestReport, NativeExecutorAction,
    NativeExecutorCancellationOutcome, NativeExecutorCancellationSignal,
    NativeExecutorCancellationState, NativeExecutorRequest, NativeExecutorResult, ObserverBatch,
    ObserverEvent, ObserverEventKind, PrivateBridgeAction, PrivateCaptureActionData,
};

use std::{collections::HashMap, ops::Deref, sync::Arc};

use localview_protocol::{
    ElementRef, EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

const PROVIDER_BOUND_GENERATION_BASE: u64 = 1 << 63;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderObserverBatch {
    pub session_id: SessionId,
    pub generation: u64,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub events: Vec<ObserverEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventSequenceGap {
    pub expected_sequence: u64,
    pub observed_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderIngestReport {
    pub ingest: IngestReport,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub continuity: EventContinuityState,
    pub gap: Option<EventSequenceGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationStatus {
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub generation: u64,
    pub last_seq: Option<u64>,
    pub event_continuity: EventContinuityState,
    pub current_snapshot_completeness: Option<ReconciliationCompleteness>,
    pub reconciliation_receipt_id: Option<String>,
    pub gap: Option<EventSequenceGap>,
}

#[derive(Debug, Clone)]
struct ProviderContinuityState {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    generation: u64,
    legacy_generation: u64,
    last_seq: Option<u64>,
    event_continuity: EventContinuityState,
    gap: Option<EventSequenceGap>,
    reconciliation: Option<ReconciliationSnapshotReceipt>,
}

impl ProviderContinuityState {
    fn new(batch: &ProviderObserverBatch) -> Self {
        Self {
            provider_incarnation_ref: batch.provider_incarnation_ref.clone(),
            target_incarnation_ref: batch.target_incarnation_ref.clone(),
            generation: batch.generation,
            legacy_generation: PROVIDER_BOUND_GENERATION_BASE,
            last_seq: None,
            event_continuity: EventContinuityState::Continuous,
            gap: None,
            reconciliation: None,
        }
    }

    fn restart_lineage(
        &mut self,
        batch: &ProviderObserverBatch,
        continuity: EventContinuityState,
    ) {
        self.provider_incarnation_ref = batch.provider_incarnation_ref.clone();
        self.target_incarnation_ref = batch.target_incarnation_ref.clone();
        self.generation = batch.generation;
        self.legacy_generation = self.legacy_generation.saturating_add(1);
        self.last_seq = None;
        self.event_continuity = continuity;
        self.gap = None;
        self.reconciliation = None;
    }

    fn observe_sequence(&mut self, sequence: u64) {
        let Some(previous) = self.last_seq else {
            self.last_seq = Some(sequence);
            return;
        };

        if sequence == previous {
            return;
        }
        if sequence < previous {
            self.event_continuity = EventContinuityState::SequenceReset;
            self.gap = None;
            self.reconciliation = None;
            return;
        }

        if sequence > previous.saturating_add(1) {
            self.event_continuity = EventContinuityState::GapDetected;
            self.gap = Some(EventSequenceGap {
                expected_sequence: previous.saturating_add(1),
                observed_sequence: sequence,
            });
            self.reconciliation = None;
        }
        self.last_seq = Some(sequence);
    }
}

#[derive(Clone, Debug)]
pub struct LiveBridge {
    legacy: legacy::LiveBridge,
    continuity: Arc<RwLock<HashMap<SessionId, ProviderContinuityState>>>,
    action_envelopes: Arc<RwLock<HashMap<Uuid, CanonicalActionEnvelope>>>,
    action_gate: Arc<Mutex<()>>,
}

impl LiveBridge {
    pub fn new(event_capacity: usize, action_capacity: usize) -> Self {
        Self {
            legacy: legacy::LiveBridge::new(event_capacity, action_capacity),
            continuity: Arc::new(RwLock::new(HashMap::new())),
            action_envelopes: Arc::new(RwLock::new(HashMap::new())),
            action_gate: Arc::new(Mutex::new(())),
        }
    }

    pub async fn ingest(&self, batch: ObserverBatch) -> IngestReport {
        self.ingest_collect(batch).await.0
    }

    pub async fn ingest_collect(
        &self,
        batch: ObserverBatch,
    ) -> (IngestReport, Vec<ObserverEvent>) {
        {
            let mut continuity = self.continuity.write().await;
            if let Some(state) = continuity.get_mut(&batch.session_id) {
                state.event_continuity = EventContinuityState::OrderingOpaque;
                state.gap = None;
                state.reconciliation = None;
            }
        }
        self.legacy.ingest_collect(batch).await
    }

    pub async fn ingest_provider(&self, batch: ProviderObserverBatch) -> ProviderIngestReport {
        let mut continuity = self.continuity.write().await;
        let state = continuity
            .entry(batch.session_id)
            .or_insert_with(|| ProviderContinuityState::new(&batch));

        if batch.provider_incarnation_ref != state.provider_incarnation_ref {
            state.restart_lineage(&batch, EventContinuityState::ProviderReincarnated);
        } else if batch.target_incarnation_ref != state.target_incarnation_ref {
            state.restart_lineage(&batch, EventContinuityState::ReconciliationRequired);
        } else if batch.generation > state.generation {
            state.restart_lineage(&batch, EventContinuityState::ReconnectedUnreconciled);
        } else if batch.generation < state.generation {
            return ProviderIngestReport {
                ingest: IngestReport {
                    accepted: 0,
                    rejected_stale: batch.events.len(),
                    last_seq: state.last_seq,
                    generation: state.generation,
                },
                provider_incarnation_ref: state.provider_incarnation_ref.clone(),
                target_incarnation_ref: state.target_incarnation_ref.clone(),
                continuity: state.event_continuity,
                gap: state.gap,
            };
        }

        for event in &batch.events {
            state.observe_sequence(event.seq);
        }

        let legacy_generation = state.legacy_generation;
        let expected_generation = state.generation;
        let provider_incarnation_ref = state.provider_incarnation_ref.clone();
        let target_incarnation_ref = state.target_incarnation_ref.clone();
        let event_continuity = state.event_continuity;
        let gap = state.gap;
        let expected_last_seq = state.last_seq;

        let (legacy_report, _) = self
            .legacy
            .ingest_collect(ObserverBatch {
                session_id: batch.session_id,
                generation: legacy_generation,
                events: batch.events,
            })
            .await;

        let ingest = IngestReport {
            accepted: legacy_report.accepted,
            rejected_stale: legacy_report.rejected_stale,
            last_seq: expected_last_seq,
            generation: expected_generation,
        };

        ProviderIngestReport {
            ingest,
            provider_incarnation_ref,
            target_incarnation_ref,
            continuity: event_continuity,
            gap,
        }
    }

    /// Compatibility enqueue for the existing web bridge. This path deliberately
    /// does not synthesize canonical authority metadata.
    pub async fn enqueue_action(
        &self,
        session_id: SessionId,
        reference: Option<ElementRef>,
        action: BridgeActionKind,
    ) -> BridgeAction {
        let _gate = self.action_gate.lock().await;
        self.legacy.enqueue_action(session_id, reference, action).await
    }

    /// Queue an action only after binding the canonical V4.3 authority envelope.
    ///
    /// This is an admission-time incarnation check, not dispatch authorization.
    /// Dispatch-time freshness/foreground/postcondition revalidation remains a
    /// later verified-action concern.
    pub async fn enqueue_canonical_action(
        &self,
        session_id: SessionId,
        reference: Option<ElementRef>,
        action: BridgeActionKind,
        metadata: ActionEnvelopeMetadata,
    ) -> Result<CanonicalQueuedAction, ActionEnvelopeBindingError> {
        if action.is_internal_capture_action() {
            return Err(ActionEnvelopeBindingError::InternalCaptureActionUnsupported);
        }
        if metadata.decision_principal_ref.as_str().trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingDecisionPrincipal);
        }
        if metadata.acting_principal_ref.as_str().trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingActingPrincipal);
        }
        if metadata.authorization_revision.trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingAuthorizationRevision);
        }
        if metadata.precondition_snapshot_cut_ref.trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingPreconditionSnapshotCut);
        }
        if metadata.risk_class != ActionRiskClass::ObserveOnly
            && metadata.expected_postcondition_contract_refs.is_empty()
        {
            return Err(ActionEnvelopeBindingError::MissingExpectedPostcondition);
        }

        let _gate = self.action_gate.lock().await;
        let current_incarnations = {
            let continuity = self.continuity.read().await;
            let Some(state) = continuity.get(&session_id) else {
                return Err(ActionEnvelopeBindingError::MissingProviderObservation);
            };
            (
                state.provider_incarnation_ref.clone(),
                state.target_incarnation_ref.clone(),
            )
        };

        if metadata.provider_incarnation_ref != current_incarnations.0 {
            return Err(ActionEnvelopeBindingError::ProviderIncarnationMismatch);
        }
        if metadata.target_incarnation_ref != current_incarnations.1 {
            return Err(ActionEnvelopeBindingError::TargetIncarnationMismatch);
        }

        let action = self
            .legacy
            .enqueue_action(session_id, reference, action)
            .await;
        let envelope = CanonicalActionEnvelope {
            envelope_id: Uuid::new_v4(),
            transport_action_id: action.id,
            session_id,
            metadata,
        };
        self.action_envelopes
            .write()
            .await
            .insert(action.id, envelope.clone());

        Ok(CanonicalQueuedAction { action, envelope })
    }

    pub async fn take_actions(&self, session_id: SessionId, limit: usize) -> Vec<BridgeAction> {
        self.take_public_actions(session_id, limit).await
    }

    pub async fn take_public_actions(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeAction> {
        let _gate = self.action_gate.lock().await;
        self.legacy.take_public_actions(session_id, limit).await
    }

    pub async fn action_envelope(&self, action_id: Uuid) -> Option<CanonicalActionEnvelope> {
        self.action_envelopes.read().await.get(&action_id).cloned()
    }

    /// Narrow freshness check for the provider/target incarnation binding only.
    /// A `true` result is never sufficient to authorize dispatch by itself.
    pub async fn action_envelope_is_current(&self, action_id: Uuid) -> bool {
        let Some(envelope) = self.action_envelope(action_id).await else {
            return false;
        };
        let continuity = self.continuity.read().await;
        let Some(state) = continuity.get(&envelope.session_id) else {
            return false;
        };
        envelope.metadata.provider_incarnation_ref == state.provider_incarnation_ref
            && envelope.metadata.target_incarnation_ref == state.target_incarnation_ref
    }

    pub async fn observation_status(&self, session_id: SessionId) -> Option<ObservationStatus> {
        let continuity = self.continuity.read().await;
        let state = continuity.get(&session_id)?;
        Some(ObservationStatus {
            provider_incarnation_ref: state.provider_incarnation_ref.clone(),
            target_incarnation_ref: state.target_incarnation_ref.clone(),
            generation: state.generation,
            last_seq: state.last_seq,
            event_continuity: state.event_continuity,
            current_snapshot_completeness: state
                .reconciliation
                .as_ref()
                .map(|receipt| receipt.completeness),
            reconciliation_receipt_id: state
                .reconciliation
                .as_ref()
                .map(|receipt| receipt.receipt_id.clone()),
            gap: state.gap,
        })
    }

    pub async fn record_reconciliation(
        &self,
        session_id: SessionId,
        receipt: ReconciliationSnapshotReceipt,
    ) -> bool {
        let mut continuity = self.continuity.write().await;
        let Some(state) = continuity.get_mut(&session_id) else {
            return false;
        };
        if receipt.provider_incarnation_ref != state.provider_incarnation_ref
            || receipt.target_incarnation_ref != state.target_incarnation_ref
        {
            return false;
        }
        state.reconciliation = Some(receipt);
        true
    }

    pub async fn release_session(&self, session_id: SessionId) {
        let _gate = self.action_gate.lock().await;
        self.continuity.write().await.remove(&session_id);
        self.action_envelopes
            .write()
            .await
            .retain(|_, envelope| envelope.session_id != session_id);
        self.legacy.release_session(session_id).await;
    }
}

impl Deref for LiveBridge {
    type Target = legacy::LiveBridge;

    fn deref(&self) -> &Self::Target {
        &self.legacy
    }
}

impl Default for LiveBridge {
    fn default() -> Self {
        Self::new(2048, 128)
    }
}
