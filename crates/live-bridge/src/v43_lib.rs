#![forbid(unsafe_code)]

#[path = "cancellable_lib.rs"]
mod legacy;

pub use legacy::{
    ActionCancellationOutcome, ActionCancellationSignal, ActionCancellationState, BridgeAction,
    BridgeActionKind, BridgeActionResult, CompletionOrigin, IngestReport, NativeExecutorAction,
    NativeExecutorCancellationOutcome, NativeExecutorCancellationSignal,
    NativeExecutorCancellationState, NativeExecutorRequest, NativeExecutorResult, ObserverBatch,
    ObserverEvent, ObserverEventKind, PrivateBridgeAction, PrivateCaptureActionData,
};

use std::{collections::HashMap, ops::Deref, sync::Arc};

use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, SessionId, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
}

impl LiveBridge {
    pub fn new(event_capacity: usize, action_capacity: usize) -> Self {
        Self {
            legacy: legacy::LiveBridge::new(event_capacity, action_capacity),
            continuity: Arc::new(RwLock::new(HashMap::new())),
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
        let mut lineage_events = Vec::with_capacity(batch.events.len());
        for event in batch.events {
            state.observe_sequence(event.seq);
            lineage_events.push((state.legacy_generation, event));
        }

        let expected_generation = state.generation;
        let provider_incarnation_ref = state.provider_incarnation_ref.clone();
        let target_incarnation_ref = state.target_incarnation_ref.clone();
        let event_continuity = state.event_continuity;
        let gap = state.gap;
        let expected_last_seq = state.last_seq;
        drop(continuity);

        let mut accepted = 0usize;
        let mut rejected_stale = 0usize;
        for (legacy_generation, event) in lineage_events {
            let (legacy_report, _) = self
                .legacy
                .ingest_collect(ObserverBatch {
                    session_id,
                    generation: legacy_generation,
                    events: vec![event],
                })
                .await;
            accepted += legacy_report.accepted;
            rejected_stale += legacy_report.rejected_stale;
        }

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
        self.continuity.write().await.remove(&session_id);
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
