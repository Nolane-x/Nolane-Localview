#![forbid(unsafe_code)]

mod action_envelope;
mod consequential_journal;
#[path = "cancellable_lib.rs"]
mod legacy;
mod postcondition_reconciliation;

pub use action_envelope::*;
pub use consequential_journal::*;
pub use legacy::{
    ActionCancellationOutcome, ActionCancellationSignal, ActionCancellationState, BridgeAction,
    BridgeActionKind, BridgeActionResult, CompletionOrigin, IngestReport, NativeExecutorAction,
    NativeExecutorCancellationOutcome, NativeExecutorCancellationSignal,
    NativeExecutorCancellationState, NativeExecutorRequest, NativeExecutorResult, ObserverBatch,
    ObserverEvent, ObserverEventKind, PrivateBridgeAction, PrivateCaptureActionData,
};
pub use postcondition_reconciliation::*;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderObservationBinding {
    pub session_id: SessionId,
    pub generation: u64,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub initial_continuity: EventContinuityState,
    pub sequence_baseline: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderObservationBindingError {
    AlreadyBound,
    UnsupportedInitialContinuity,
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

    fn from_binding(binding: &ProviderObservationBinding) -> Self {
        Self {
            provider_incarnation_ref: binding.provider_incarnation_ref.clone(),
            target_incarnation_ref: binding.target_incarnation_ref.clone(),
            generation: binding.generation,
            legacy_generation: PROVIDER_BOUND_GENERATION_BASE,
            last_seq: binding.sequence_baseline,
            event_continuity: binding.initial_continuity,
            gap: None,
            reconciliation: None,
        }
    }

    fn status(&self) -> ObservationStatus {
        ObservationStatus {
            provider_incarnation_ref: self.provider_incarnation_ref.clone(),
            target_incarnation_ref: self.target_incarnation_ref.clone(),
            generation: self.generation,
            last_seq: self.last_seq,
            event_continuity: self.event_continuity,
            current_snapshot_completeness: self
                .reconciliation
                .as_ref()
                .map(|receipt| receipt.completeness),
            reconciliation_receipt_id: self
                .reconciliation
                .as_ref()
                .map(|receipt| receipt.receipt_id.clone()),
            gap: self.gap,
        }
    }

    fn restart_lineage(&mut self, batch: &ProviderObserverBatch, continuity: EventContinuityState) {
        self.provider_incarnation_ref = batch.provider_incarnation_ref.clone();
        self.target_incarnation_ref = batch.target_incarnation_ref.clone();
        self.generation = batch.generation;
        self.legacy_generation = self.legacy_generation.saturating_add(1);
        self.last_seq = None;
        self.event_continuity = continuity;
        self.gap = None;
        self.reconciliation = None;
    }

    /// Observe one provider sequence number and return true when the provider has
    /// reset its sequence baseline within the same declared incarnation.
    ///
    /// A reset opens a fresh internal bounded-buffer lineage but deliberately
    /// preserves `SEQUENCE_RESET` as the public continuity state. It must never be
    /// laundered into `CONTINUOUS` merely because subsequent reset-lineage events
    /// are locally contiguous.
    fn observe_sequence(&mut self, sequence: u64) -> bool {
        let Some(previous) = self.last_seq else {
            self.last_seq = Some(sequence);
            return false;
        };

        if sequence == previous {
            return false;
        }
        if sequence < previous {
            self.legacy_generation = self.legacy_generation.saturating_add(1);
            self.last_seq = Some(sequence);
            self.event_continuity = EventContinuityState::SequenceReset;
            self.gap = None;
            self.reconciliation = None;
            return true;
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
        false
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

    /// Bind provider reliability before the first provider event is ingested.
    ///
    /// Only declarative starting states are accepted. Evidence-backed adverse
    /// states such as a gap, sequence reset, provider reincarnation, reconnect,
    /// or broken stream must be produced by observation rather than asserted by
    /// a provider. Existing session state is never overwritten, preventing a
    /// second binding from laundering continuity debt.
    pub async fn bind_provider_observation(
        &self,
        binding: ProviderObservationBinding,
    ) -> Result<ObservationStatus, ProviderObservationBindingError> {
        if !matches!(
            binding.initial_continuity,
            EventContinuityState::Continuous
                | EventContinuityState::OrderingOpaque
                | EventContinuityState::ReconciliationRequired
        ) {
            return Err(ProviderObservationBindingError::UnsupportedInitialContinuity);
        }

        let mut continuity = self.continuity.write().await;
        if continuity.contains_key(&binding.session_id) {
            return Err(ProviderObservationBindingError::AlreadyBound);
        }

        let state = ProviderContinuityState::from_binding(&binding);
        let status = state.status();
        continuity.insert(binding.session_id, state);
        Ok(status)
    }

    pub async fn ingest(&self, batch: ObserverBatch) -> IngestReport {
        self.ingest_collect(batch).await.0
    }

    pub async fn ingest_collect(&self, batch: ObserverBatch) -> (IngestReport, Vec<ObserverEvent>) {
        // The legacy web stream has no authority over provider-bound event
        // continuity or reconciliation. Mixing the two state domains here would
        // allow an unrelated web batch to erase an observed provider gap.
        self.legacy.ingest_collect(batch).await
    }

    pub async fn ingest_provider(&self, batch: ProviderObserverBatch) -> ProviderIngestReport {
        let session_id = batch.session_id;
        let mut continuity = self.continuity.write().await;
        let state = continuity
            .entry(session_id)
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

        // Bind each event to the exact internal bounded-buffer lineage that was
        // current when its sequence was observed. This matters when a provider
        // resets sequence numbers in the middle of a batch: feeding the whole
        // batch under one legacy generation would either retain stale pre-reset
        // events or reject the new low sequence as stale.
        let mut accepted = 0usize;
        let mut rejected_stale = 0usize;
        for event in batch.events {
            state.observe_sequence(event.seq);
            let (legacy_report, _) = self
                .legacy
                .ingest_collect(ObserverBatch {
                    session_id,
                    generation: state.legacy_generation,
                    events: vec![event],
                })
                .await;
            accepted += legacy_report.accepted;
            rejected_stale += legacy_report.rejected_stale;
        }

        let expected_generation = state.generation;
        let provider_incarnation_ref = state.provider_incarnation_ref.clone();
        let target_incarnation_ref = state.target_incarnation_ref.clone();
        let event_continuity = state.event_continuity;
        let gap = state.gap;
        let expected_last_seq = state.last_seq;

        let ingest = IngestReport {
            accepted,
            rejected_stale,
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
        self.legacy
            .enqueue_action(session_id, reference, action)
            .await
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
        Some(state.status())
    }

    pub(crate) async fn current_reconciliation_snapshot(
        &self,
        session_id: SessionId,
    ) -> Option<ReconciliationSnapshotReceipt> {
        self.continuity
            .read()
            .await
            .get(&session_id)
            .and_then(|state| state.reconciliation.clone())
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

    /// Remove only provider-bound observation authority for one session.
    ///
    /// This deliberately preserves legacy web history, action results, and
    /// canonical envelope evidence. The action gate prevents a canonical action
    /// from being admitted concurrently against observation authority that is
    /// being detached.
    pub async fn release_provider_observation(&self, session_id: SessionId) -> bool {
        let _gate = self.action_gate.lock().await;
        self.continuity.write().await.remove(&session_id).is_some()
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
