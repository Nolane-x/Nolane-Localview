#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::{ElementRef, SessionId, ViewportMeta};
use localview_token_budget::{
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetUsage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

const MAX_PRIVATE_MASK_SELECTORS: usize = 16;
const MAX_PRIVATE_MASK_SELECTOR_BYTES: usize = 256;
const MAX_VISUAL_MASK_RECTS: usize = 256;
const MAX_MASKED_ELEMENTS: u64 = 4_096;
const MAX_POSITIONAL_SCAN_ELEMENTS: u64 = 4_096;
const MAX_CSS_VIEWPORT_DIMENSION: f64 = 100_000.0;
const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;
const VIEWPORT_VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;
const MAX_NATIVE_EXECUTOR_RESULT_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_NATIVE_EXECUTOR_ERROR_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ObserverEventKind {
    DomMutation,
    Layout,
    Route,
    Focus,
    Scroll,
    Console,
    Network,
    RuntimeError,
    Performance,
    Hmr,
    SemanticSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObserverEvent {
    pub seq: u64,
    pub captured_at: DateTime<Utc>,
    pub kind: ObserverEventKind,
    pub reference: Option<ElementRef>,
    pub route: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObserverBatch {
    pub session_id: SessionId,
    pub generation: u64,
    pub events: Vec<ObserverEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IngestReport {
    pub accepted: usize,
    pub rejected_stale: usize,
    pub last_seq: Option<u64>,
    pub generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeActionKind {
    Click,
    TypeText { text: String, clear_first: bool },
    Key { key: String, modifiers: Vec<String> },
    Scroll { x: f64, y: f64 },
    Focus,
    Snapshot,
    Measure,
    CssInspect,
    FreezeVisuals,
    RestoreVisuals { token: Uuid },
    CaptureScrollTo { token: Uuid, y: f64 },
    CaptureTileProbe { token: Uuid },
}

impl BridgeActionKind {
    pub fn is_internal_capture_action(&self) -> bool {
        matches!(
            self,
            Self::FreezeVisuals
                | Self::RestoreVisuals { .. }
                | Self::CaptureScrollTo { .. }
                | Self::CaptureTileProbe { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateCaptureActionData {
    pub mask_selectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_freeze_lease_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeAction {
    pub id: Uuid,
    pub session_id: SessionId,
    pub reference: Option<ElementRef>,
    pub action: BridgeActionKind,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrivateBridgeAction {
    pub id: Uuid,
    pub session_id: SessionId,
    pub reference: Option<ElementRef>,
    pub action: BridgeActionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_capture: Option<PrivateCaptureActionData>,
    pub created_at: DateTime<Utc>,
}

impl PrivateBridgeAction {
    fn from_action(action: BridgeAction, private_capture: Option<PrivateCaptureActionData>) -> Self {
        Self {
            id: action.id,
            session_id: action.session_id,
            reference: action.reference,
            action: action.action,
            private_capture,
            created_at: action.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeActionResult {
    pub action_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionExecutionBoundary {
    pub action_id: Uuid,
    pub session_id: SessionId,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeExecutorAction {
    VisualPacket {
        reference: Option<ElementRef>,
        viewport: ViewportMeta,
        revision: Option<String>,
        budget: PerceptionBudgetContract,
        budget_escalation_reason: Option<BudgetEscalationReason>,
    },
    VisualDiffCapture {
        viewport: ViewportMeta,
        revision: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeExecutorRequest {
    pub id: Uuid,
    pub session_id: SessionId,
    pub action: NativeExecutorAction,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeExecutorResult {
    pub request_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
    pub usage: Option<PerceptionBudgetUsage>,
    #[serde(default)]
    pub payload: Value,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NetworkFaultControlCommand {
    Install {
        lease_token: Uuid,
        plan: Value,
    },
    Clear {
        lease_token: Uuid,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkFaultControlRequest {
    pub id: Uuid,
    pub session_id: SessionId,
    pub command: NetworkFaultControlCommand,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetworkFaultControlResult {
    pub request_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkFaultLeaseAuthority {
    pub lease_id: Uuid,
    pub lease_token: Uuid,
    pub fingerprint: String,
    pub rule_count: usize,
    pub surface_incarnation: u64,
    pub expires_at: DateTime<Utc>,
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum CompletionOrigin {
    Session(SessionId),
    Action(BridgeAction),
}

impl From<SessionId> for CompletionOrigin {
    fn from(session_id: SessionId) -> Self {
        Self::Session(session_id)
    }
}

impl From<&BridgeAction> for CompletionOrigin {
    fn from(action: &BridgeAction) -> Self {
        Self::Action(action.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionScope {
    Public,
    InternalCapture,
}

impl ActionScope {
    fn from_action(action: &BridgeAction) -> Self {
        if action.action.is_internal_capture_action() {
            Self::InternalCapture
        } else {
            Self::Public
        }
    }
}

#[derive(Debug, Default)]
struct SessionBridgeState {
    generation: u64,
    last_seq: Option<u64>,
    events: VecDeque<ObserverEvent>,
    actions: VecDeque<BridgeAction>,
    capture_actions: VecDeque<BridgeAction>,
    capture_private: VecDeque<(Uuid, PrivateCaptureActionData)>,
    inflight: VecDeque<BridgeAction>,
    capture_inflight: VecDeque<BridgeAction>,
    claimed: VecDeque<BridgeAction>,
    capture_claimed: VecDeque<BridgeAction>,
    action_started_at: HashMap<Uuid, DateTime<Utc>>,
    action_boundaries: VecDeque<ActionExecutionBoundary>,
    results: VecDeque<BridgeActionResult>,
    capture_results: VecDeque<BridgeActionResult>,
    native_executor_requests: VecDeque<NativeExecutorRequest>,
    native_executor_inflight: VecDeque<NativeExecutorRequest>,
    native_executor_claimed: VecDeque<NativeExecutorRequest>,
    native_executor_results: VecDeque<NativeExecutorResult>,
    network_fault_requests: VecDeque<NetworkFaultControlRequest>,
    network_fault_inflight: VecDeque<NetworkFaultControlRequest>,
    network_fault_claimed: VecDeque<NetworkFaultControlRequest>,
    network_fault_results: VecDeque<NetworkFaultControlResult>,
    network_fault_lease: Option<NetworkFaultLeaseAuthority>,
}

#[derive(Clone, Debug)]
pub struct LiveBridge {
    inner: Arc<RwLock<HashMap<SessionId, SessionBridgeState>>>,
    event_capacity: usize,
    action_capacity: usize,
    result_capacity: usize,
}

impl LiveBridge {
    pub fn new(event_capacity: usize, action_capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            event_capacity: event_capacity.max(32),
            action_capacity: action_capacity.max(8),
            result_capacity: action_capacity.max(8),
        }
    }

    pub async fn ingest(&self, batch: ObserverBatch) -> IngestReport {
        self.ingest_collect(batch).await.0
    }

    pub async fn ingest_collect(
        &self,
        batch: ObserverBatch,
    ) -> (IngestReport, Vec<ObserverEvent>) {
        let mut states = self.inner.write().await;
        let state = states.entry(batch.session_id).or_default();
        if batch.generation > state.generation {
            state.generation = batch.generation;
            state.last_seq = None;
            state.events.clear();
        }
        let mut accepted_events = Vec::new();
        let mut rejected_stale = 0;
        for event in batch.events {
            let stale_generation = batch.generation < state.generation;
            let stale_sequence = state.last_seq.is_some_and(|seq| event.seq <= seq);
            if stale_generation || stale_sequence {
                rejected_stale += 1;
                continue;
            }
            state.last_seq = Some(event.seq);
            accepted_events.push(event.clone());
            state.events.push_back(event);
            while state.events.len() > self.event_capacity {
                state.events.pop_front();
            }
        }
        let report = IngestReport {
            accepted: accepted_events.len(),
            rejected_stale,
            last_seq: state.last_seq,
            generation: state.generation,
        };
        (report, accepted_events)
    }

    pub async fn recent(&self, session_id: SessionId, limit: usize) -> Vec<ObserverEvent> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| recent_from(&state.events, limit))
            .unwrap_or_default()
    }

    pub async fn enqueue_action(
        &self,
        session_id: SessionId,
        reference: Option<ElementRef>,
        action: BridgeActionKind,
    ) -> BridgeAction {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id,
            reference,
            action,
            created_at: Utc::now(),
        };
        let scope = ActionScope::from_action(&action);
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        match scope {
            ActionScope::Public => push_bounded(&mut state.actions, action.clone(), self.action_capacity),
            ActionScope::InternalCapture => push_bounded(
                &mut state.capture_actions,
                action.clone(),
                self.action_capacity,
            ),
        }
        action
    }

    pub async fn enqueue_capture_freeze(
        &self,
        session_id: SessionId,
        mask_selectors: Vec<String>,
    ) -> BridgeAction {
        self.enqueue_capture_freeze_with_lease(
            session_id,
            mask_selectors,
            VIEWPORT_VISUAL_FREEZE_LEASE_MS,
        )
        .await
    }

    pub async fn enqueue_full_page_capture_freeze(
        &self,
        session_id: SessionId,
        mask_selectors: Vec<String>,
    ) -> BridgeAction {
        self.enqueue_capture_freeze_with_lease(
            session_id,
            mask_selectors,
            FULL_PAGE_VISUAL_FREEZE_LEASE_MS,
        )
        .await
    }

    async fn enqueue_capture_freeze_with_lease(
        &self,
        session_id: SessionId,
        mask_selectors: Vec<String>,
        visual_freeze_lease_ms: u64,
    ) -> BridgeAction {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id,
            reference: None,
            action: BridgeActionKind::FreezeVisuals,
            created_at: Utc::now(),
        };
        let private = PrivateCaptureActionData {
            mask_selectors: sanitize_mask_selectors(mask_selectors),
            visual_freeze_lease_ms: Some(visual_freeze_lease_ms),
        };
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        push_bounded(
            &mut state.capture_actions,
            action.clone(),
            self.action_capacity,
        );
        push_bounded(
            &mut state.capture_private,
            (action.id, private),
            self.action_capacity,
        );
        action
    }

    pub async fn enqueue_capture_tile_probe(
        &self,
        session_id: SessionId,
        token: Uuid,
        mask_selectors: Vec<String>,
    ) -> BridgeAction {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id,
            reference: None,
            action: BridgeActionKind::CaptureTileProbe { token },
            created_at: Utc::now(),
        };
        let private = PrivateCaptureActionData {
            mask_selectors: sanitize_mask_selectors(mask_selectors),
            visual_freeze_lease_ms: None,
        };
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        push_bounded(
            &mut state.capture_actions,
            action.clone(),
            self.action_capacity,
        );
        push_bounded(
            &mut state.capture_private,
            (action.id, private),
            self.action_capacity,
        );
        action
    }

    pub async fn enqueue_native_executor(
        &self,
        session_id: SessionId,
        action: NativeExecutorAction,
    ) -> NativeExecutorRequest {
        let request = NativeExecutorRequest {
            id: Uuid::new_v4(),
            session_id,
            action,
            created_at: Utc::now(),
        };
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        push_bounded(
            &mut state.native_executor_requests,
            request.clone(),
            self.action_capacity,
        );
        request
    }

    pub async fn enqueue_network_fault_control(
        &self,
        session_id: SessionId,
        command: NetworkFaultControlCommand,
    ) -> NetworkFaultControlRequest {
        let request = NetworkFaultControlRequest {
            id: Uuid::new_v4(),
            session_id,
            command,
            created_at: Utc::now(),
        };
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        push_bounded(
            &mut state.network_fault_requests,
            request.clone(),
            self.action_capacity,
        );
        request
    }

    pub async fn take_network_fault_controls(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NetworkFaultControlRequest> {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };
        let active = state
            .network_fault_inflight
            .len()
            .saturating_add(state.network_fault_claimed.len());
        let available = self.action_capacity.saturating_sub(active);
        let count = limit
            .min(state.network_fault_requests.len())
            .min(available);
        let requests = state
            .network_fault_requests
            .drain(..count)
            .collect::<Vec<_>>();
        for request in &requests {
            push_bounded(
                &mut state.network_fault_inflight,
                request.clone(),
                self.action_capacity,
            );
        }
        requests
    }

    pub async fn claim_network_fault_control(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NetworkFaultControlRequest> {
        let mut states = self.inner.write().await;
        let state = states.get_mut(&session_id)?;
        let index = state
            .network_fault_inflight
            .iter()
            .position(|request| request.id == request_id && request.session_id == session_id)?;
        let request = state.network_fault_inflight.remove(index)?;
        push_bounded(
            &mut state.network_fault_claimed,
            request.clone(),
            self.action_capacity,
        );
        Some(request)
    }

    pub async fn complete_network_fault_control(
        &self,
        session_id: SessionId,
        mut result: NetworkFaultControlResult,
    ) -> bool {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return false;
        };
        let Some(index) = state
            .network_fault_claimed
            .iter()
            .position(|request| {
                request.id == result.request_id && request.session_id == session_id
            })
        else {
            return false;
        };
        let Some(request) = state.network_fault_claimed.remove(index) else {
            return false;
        };
        sanitize_network_fault_control_result(&request, &mut result);
        push_bounded(
            &mut state.network_fault_results,
            result,
            self.result_capacity,
        );
        true
    }

    pub async fn network_fault_control_result(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NetworkFaultControlResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)?
            .network_fault_results
            .iter()
            .rev()
            .find(|result| result.request_id == request_id)
            .cloned()
    }

    pub async fn set_network_fault_lease(
        &self,
        session_id: SessionId,
        lease: NetworkFaultLeaseAuthority,
    ) {
        let mut states = self.inner.write().await;
        states.entry(session_id).or_default().network_fault_lease = Some(lease);
    }

    pub async fn network_fault_lease(
        &self,
        session_id: SessionId,
    ) -> Option<NetworkFaultLeaseAuthority> {
        self.inner
            .read()
            .await
            .get(&session_id)
            .and_then(|state| state.network_fault_lease.clone())
    }

    pub async fn clear_network_fault_lease(
        &self,
        session_id: SessionId,
        lease_id: Uuid,
    ) -> bool {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return false;
        };
        if state
            .network_fault_lease
            .as_ref()
            .is_some_and(|lease| lease.lease_id == lease_id)
        {
            state.network_fault_lease = None;
            true
        } else {
            false
        }
    }

    pub async fn take_actions(&self, session_id: SessionId, limit: usize) -> Vec<BridgeAction> {
        self.take_public_actions(session_id, limit).await
    }

    pub async fn take_public_actions(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeAction> {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };
        let actions = drain_actions(
            &mut state.actions,
            &mut state.inflight,
            limit,
            self.action_capacity,
        );
        let started_at = Utc::now();
        for action in &actions {
            state.action_started_at.entry(action.id).or_insert(started_at);
        }
        actions
    }

    pub async fn take_internal_capture_actions(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<PrivateBridgeAction> {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };
        let actions = drain_actions(
            &mut state.capture_actions,
            &mut state.capture_inflight,
            limit,
            self.action_capacity,
        );
        actions
            .into_iter()
            .map(|action| {
                let private_capture = take_private_capture(&mut state.capture_private, action.id);
                PrivateBridgeAction::from_action(action, private_capture)
            })
            .collect()
    }

    pub async fn take_native_executor_requests(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorRequest> {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };
        let active = state
            .native_executor_inflight
            .len()
            .saturating_add(state.native_executor_claimed.len());
        let available = self.action_capacity.saturating_sub(active);
        let count = limit
            .min(state.native_executor_requests.len())
            .min(available);
        let requests = state
            .native_executor_requests
            .drain(..count)
            .collect::<Vec<_>>();
        for request in &requests {
            state.native_executor_inflight.push_back(request.clone());
        }
        requests
    }

    pub async fn claim_action(
        &self,
        session_id: SessionId,
        action_id: Uuid,
    ) -> Option<BridgeAction> {
        let mut states = self.inner.write().await;
        let state = states.get_mut(&session_id)?;

        if let Some(action) = claim_from_scope(
            &mut state.capture_inflight,
            &mut state.capture_claimed,
            action_id,
            self.action_capacity,
        ) {
            return Some(action);
        }
        claim_from_scope(
            &mut state.inflight,
            &mut state.claimed,
            action_id,
            self.action_capacity,
        )
    }

    pub async fn discard_public_action(&self, session_id: SessionId, action_id: Uuid) -> bool {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return false;
        };
        let Some(index) = state.inflight.iter().position(|action| action.id == action_id) else {
            return false;
        };
        let removed = state.inflight.remove(index).is_some();
        if removed {
            state.action_started_at.remove(&action_id);
        }
        removed
    }

    pub async fn claim_native_executor(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorRequest> {
        let mut states = self.inner.write().await;
        let state = states.get_mut(&session_id)?;
        let index = state
            .native_executor_inflight
            .iter()
            .position(|request| request.id == request_id)?;
        let request = state.native_executor_inflight.remove(index)?;
        state.native_executor_claimed.push_back(request.clone());
        Some(request)
    }

    pub async fn expire_native_executor_active_before(
        &self,
        session_id: SessionId,
        cutoff: DateTime<Utc>,
    ) -> usize {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return 0;
        };

        let before = state
            .native_executor_inflight
            .len()
            .saturating_add(state.native_executor_claimed.len());
        state
            .native_executor_inflight
            .retain(|request| request.created_at >= cutoff);
        state
            .native_executor_claimed
            .retain(|request| request.created_at >= cutoff);
        let after = state
            .native_executor_inflight
            .len()
            .saturating_add(state.native_executor_claimed.len());
        before.saturating_sub(after)
    }

    pub async fn complete_action(
        &self,
        origin: impl Into<CompletionOrigin>,
        mut result: BridgeActionResult,
    ) {
        let mut states = self.inner.write().await;
        let origin = origin.into();
        let session_id = match &origin {
            CompletionOrigin::Session(session_id) => *session_id,
            CompletionOrigin::Action(action) => action.session_id,
        };
        let state = states.entry(session_id).or_default();

        let completed = match origin {
            CompletionOrigin::Action(action) => {
                let scope = ActionScope::from_action(&action);
                remove_claimed_for_scope(state, scope, action.id);
                Some((action, scope))
            }
            CompletionOrigin::Session(_) => take_claimed_by_id(state, result.action_id),
        };

        let daemon_completed_at = Utc::now();
        if let Some((action, scope)) = completed.as_ref() {
            if *scope == ActionScope::Public {
                if let Some(started_at) = state.action_started_at.remove(&action.id) {
                    push_bounded(
                        &mut state.action_boundaries,
                        ActionExecutionBoundary {
                            action_id: action.id,
                            session_id,
                            started_at,
                            completed_at: daemon_completed_at,
                        },
                        self.result_capacity,
                    );
                }
            }
        }

        sanitize_result_for_storage(completed.as_ref().map(|(action, _)| action), &mut result);
        match completed.map(|(_, scope)| scope).unwrap_or(ActionScope::Public) {
            ActionScope::Public => push_bounded(&mut state.results, result, self.result_capacity),
            ActionScope::InternalCapture => {
                push_bounded(&mut state.capture_results, result, self.result_capacity)
            }
        }
    }

    pub async fn complete_native_executor(
        &self,
        session_id: SessionId,
        mut result: NativeExecutorResult,
    ) -> bool {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return false;
        };
        let Some(index) = state
            .native_executor_claimed
            .iter()
            .position(|request| request.id == result.request_id && request.session_id == session_id)
        else {
            return false;
        };
        let Some(request) = state.native_executor_claimed.remove(index) else {
            return false;
        };
        sanitize_native_executor_result(&request, &mut result);
        push_bounded(
            &mut state.native_executor_results,
            result,
            self.result_capacity,
        );
        true
    }

    pub async fn recent_results(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeActionResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| recent_from(&state.results, limit))
            .unwrap_or_default()
    }

    pub async fn recent_internal_capture_results(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeActionResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| recent_from(&state.capture_results, limit))
            .unwrap_or_default()
    }

    pub async fn native_executor_result(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)?
            .native_executor_results
            .iter()
            .find(|result| result.request_id == request_id)
            .cloned()
    }

    pub async fn recent_native_executor_results(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| recent_from(&state.native_executor_results, limit))
            .unwrap_or_default()
    }

    pub async fn action_execution_boundary(
        &self,
        session_id: SessionId,
        action_id: Uuid,
    ) -> Option<ActionExecutionBoundary> {
        let states = self.inner.read().await;
        states
            .get(&session_id)?
            .action_boundaries
            .iter()
            .rev()
            .find(|boundary| boundary.action_id == action_id)
            .cloned()
    }

    pub async fn recent_action_execution_boundaries(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<ActionExecutionBoundary> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| recent_from(&state.action_boundaries, limit))
            .unwrap_or_default()
    }

    pub async fn release_session(&self, session_id: SessionId) {
        self.inner.write().await.remove(&session_id);
    }
}

fn sanitize_mask_selectors(mask_selectors: Vec<String>) -> Vec<String> {
    mask_selectors
        .into_iter()
        .filter(|selector| !selector.is_empty() && selector.len() <= MAX_PRIVATE_MASK_SELECTOR_BYTES)
        .take(MAX_PRIVATE_MASK_SELECTORS)
        .collect()
}

fn take_private_capture(
    private: &mut VecDeque<(Uuid, PrivateCaptureActionData)>,
    action_id: Uuid,
) -> Option<PrivateCaptureActionData> {
    let index = private.iter().position(|(id, _)| *id == action_id)?;
    private.remove(index).map(|(_, data)| data)
}

fn drain_actions(
    queue: &mut VecDeque<BridgeAction>,
    inflight: &mut VecDeque<BridgeAction>,
    limit: usize,
    capacity: usize,
) -> Vec<BridgeAction> {
    let count = limit.min(queue.len());
    let actions = queue.drain(..count).collect::<Vec<_>>();
    for action in &actions {
        push_bounded(inflight, action.clone(), capacity);
    }
    actions
}

fn claim_from_scope(
    inflight: &mut VecDeque<BridgeAction>,
    claimed: &mut VecDeque<BridgeAction>,
    action_id: Uuid,
    capacity: usize,
) -> Option<BridgeAction> {
    let index = inflight.iter().position(|action| action.id == action_id)?;
    let action = inflight.remove(index)?;
    push_bounded(claimed, action.clone(), capacity);
    Some(action)
}

fn remove_claimed_for_scope(state: &mut SessionBridgeState, scope: ActionScope, action_id: Uuid) {
    let claimed = match scope {
        ActionScope::Public => &mut state.claimed,
        ActionScope::InternalCapture => &mut state.capture_claimed,
    };
    if let Some(index) = claimed.iter().position(|action| action.id == action_id) {
        claimed.remove(index);
    }
}

fn take_claimed_by_id(
    state: &mut SessionBridgeState,
    action_id: Uuid,
) -> Option<(BridgeAction, ActionScope)> {
    if let Some(index) = state
        .capture_claimed
        .iter()
        .position(|action| action.id == action_id)
    {
        return state
            .capture_claimed
            .remove(index)
            .map(|action| (action, ActionScope::InternalCapture));
    }
    let index = state.claimed.iter().position(|action| action.id == action_id)?;
    state
        .claimed
        .remove(index)
        .map(|action| (action, ActionScope::Public))
}

fn push_bounded<T>(queue: &mut VecDeque<T>, value: T, capacity: usize) {
    queue.push_back(value);
    while queue.len() > capacity {
        queue.pop_front();
    }
}

fn recent_from<T: Clone>(queue: &VecDeque<T>, limit: usize) -> Vec<T> {
    queue
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn sanitize_network_fault_control_result(
    request: &NetworkFaultControlRequest,
    result: &mut NetworkFaultControlResult,
) {
    fn fail(result: &mut NetworkFaultControlResult) {
        result.ok = false;
        result.payload = Value::Null;
        result.error = Some("network fault private control failed".into());
    }

    if result.request_id != request.id || !result.ok {
        fail(result);
        return;
    }

    let Some(input) = result.payload.as_object() else {
        fail(result);
        return;
    };
    let Some(active) = input.get("active").and_then(Value::as_bool) else {
        fail(result);
        return;
    };

    let mut output = serde_json::Map::new();
    output.insert("active".into(), Value::Bool(active));

    for key in ["installed", "cleared"] {
        if let Some(value) = input.get(key).and_then(Value::as_bool) {
            output.insert(key.into(), Value::Bool(value));
        }
    }

    if let Some(fingerprint) = input.get("fingerprint").and_then(Value::as_str) {
        if fingerprint.len() != 16
            || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            fail(result);
            return;
        }
        output.insert("fingerprint".into(), Value::String(fingerprint.to_ascii_lowercase()));
    }

    for (key, max) in [
        ("rule_count", 16_u64),
        ("total_hits", 1_024_u64),
        ("remaining_ms", 30_000_u64),
        ("expires_in_ms", 30_000_u64),
        ("surface_incarnation", u64::MAX),
    ] {
        if let Some(value) = input.get(key) {
            let Some(value) = value.as_u64() else {
                fail(result);
                return;
            };
            if value > max {
                fail(result);
                return;
            }
            output.insert(key.into(), Value::from(value));
        }
    }

    result.payload = Value::Object(output);
    result.error = None;
}

fn sanitize_native_executor_result(
    request: &NativeExecutorRequest,
    result: &mut NativeExecutorResult,
) {
    if result.request_id != request.id {
        result.ok = false;
        result.usage = None;
        result.payload = Value::Null;
        result.error = Some("native executor result origin mismatch".into());
        return;
    }

    if serde_json::to_vec(&result.payload)
        .map(|bytes| bytes.len() > MAX_NATIVE_EXECUTOR_RESULT_PAYLOAD_BYTES)
        .unwrap_or(true)
    {
        result.ok = false;
        result.usage = None;
        result.payload = Value::Null;
        result.error = Some("native executor result payload exceeded bounded metadata limit".into());
    }

    if let Some(error) = result.error.as_mut() {
        if error.len() > MAX_NATIVE_EXECUTOR_ERROR_BYTES {
            let mut end = MAX_NATIVE_EXECUTOR_ERROR_BYTES;
            while end > 0 && !error.is_char_boundary(end) {
                end -= 1;
            }
            error.truncate(end);
        }
    }
}

fn sanitize_result_for_storage(action: Option<&BridgeAction>, result: &mut BridgeActionResult) {
    if let Some(action) = action {
        if matches!(action.action, BridgeActionKind::Measure) {
            sanitize_measure_result(action, result);
            return;
        }
    }

    match action.map(|action| &action.action) {
        Some(BridgeActionKind::TypeText { text, .. }) => {
            result.payload = Value::Null;
            if !text.is_empty() {
                result.error = result
                    .error
                    .take()
                    .map(|error| error.replace(text, "[REDACTED]"));
            }
        }
        Some(BridgeActionKind::FreezeVisuals) => sanitize_visual_freeze_result(result),
        Some(BridgeActionKind::CaptureScrollTo { .. }) => sanitize_capture_scroll_result(result),
        Some(BridgeActionKind::CaptureTileProbe { .. }) => sanitize_capture_tile_probe_result(result),
        Some(BridgeActionKind::RestoreVisuals { .. }) => {
            result.payload = Value::Null;
            if result.error.is_some() {
                result.error = Some("visual restore action failed".into());
            }
        }
        Some(_) => {}
        None => {
            result.payload = Value::Null;
            if result.error.is_some() {
                result.error = Some("[REDACTED: action origin unavailable]".into());
            }
        }
    }
}

fn sanitize_measure_result(action: &BridgeAction, result: &mut BridgeActionResult) {
    const MAX_MEASURE_REFERENCE_BYTES: usize = 64;
    const MAX_MEASURE_ROUTE_BYTES: usize = 2_048;
    const MAX_MEASURE_ABS_COORDINATE: f64 = 1_000_000.0;
    const MEASURE_DIMENSION_TOLERANCE: f64 = 0.2;

    fn rect(value: Option<&Value>) -> Option<Value> {
        let value = value?.as_object()?;
        let x = value.get("x")?.as_f64()?;
        let y = value.get("y")?.as_f64()?;
        let width = value.get("width")?.as_f64()?;
        let height = value.get("height")?.as_f64()?;
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || x.abs() > MAX_MEASURE_ABS_COORDINATE
            || y.abs() > MAX_MEASURE_ABS_COORDINATE
            || width < 0.0
            || height < 0.0
            || width > MAX_CSS_VIEWPORT_DIMENSION
            || height > MAX_CSS_VIEWPORT_DIMENSION
            || !(x + width).is_finite()
            || !(y + height).is_finite()
        {
            return None;
        }
        Some(serde_json::json!({
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }))
    }

    fn fail(result: &mut BridgeActionResult) {
        result.ok = false;
        result.payload = Value::Null;
        result.error = Some("measure action failed".into());
    }

    if !result.ok {
        result.payload = Value::Null;
        if result.error.is_some() {
            result.error = Some("measure action failed".into());
        }
        return;
    }

    let Some(expected_reference) = action.reference.as_deref() else {
        fail(result);
        return;
    };
    let Some(reference) = result.payload.get("reference").and_then(Value::as_str) else {
        fail(result);
        return;
    };
    let Some(reference_hash) = reference.strip_prefix("@e") else {
        fail(result);
        return;
    };
    if reference != expected_reference
        || reference.len() > MAX_MEASURE_REFERENCE_BYTES
        || reference_hash.is_empty()
        || !reference_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        fail(result);
        return;
    }

    let Some(viewport) = result.payload.get("viewport").and_then(Value::as_object) else {
        fail(result);
        return;
    };
    let Some(viewport_width) = viewport.get("width").and_then(Value::as_f64) else {
        fail(result);
        return;
    };
    let Some(viewport_height) = viewport.get("height").and_then(Value::as_f64) else {
        fail(result);
        return;
    };
    if !valid_positive_css_dimension(viewport_width)
        || !valid_positive_css_dimension(viewport_height)
    {
        fail(result);
        return;
    }

    let Some(viewport_rect) = rect(result.payload.get("rect")) else {
        fail(result);
        return;
    };
    let Some(document_rect) = rect(result.payload.get("document_rect")) else {
        fail(result);
        return;
    };
    let viewport_rect_width = viewport_rect.get("width").and_then(Value::as_f64).unwrap_or(-1.0);
    let viewport_rect_height = viewport_rect.get("height").and_then(Value::as_f64).unwrap_or(-1.0);
    let document_rect_width = document_rect.get("width").and_then(Value::as_f64).unwrap_or(-1.0);
    let document_rect_height = document_rect.get("height").and_then(Value::as_f64).unwrap_or(-1.0);
    if (viewport_rect_width - document_rect_width).abs() > MEASURE_DIMENSION_TOLERANCE
        || (viewport_rect_height - document_rect_height).abs() > MEASURE_DIMENSION_TOLERANCE
    {
        fail(result);
        return;
    }

    let Some(route) = result.payload.get("route").and_then(Value::as_str) else {
        fail(result);
        return;
    };
    if route.len() > MAX_MEASURE_ROUTE_BYTES {
        fail(result);
        return;
    }
    let Ok(mut route_url) = url::Url::parse(route) else {
        fail(result);
        return;
    };
    if !matches!(route_url.scheme(), "http" | "https") {
        fail(result);
        return;
    }
    let Some(host) = route_url.host_str() else {
        fail(result);
        return;
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false);
    if !loopback {
        fail(result);
        return;
    }
    route_url.set_query(None);
    route_url.set_fragment(None);

    result.payload = serde_json::json!({
        "reference": reference,
        "rect": viewport_rect,
        "document_rect": document_rect,
        "viewport": {
            "width": viewport_width,
            "height": viewport_height,
        },
        "route": route_url.to_string(),
    });
    result.error = None;
}

fn sanitize_visual_freeze_result(result: &mut BridgeActionResult) {
    if !result.ok {
        fail_internal_capture_result(result, "visual_freeze_failed");
        return;
    }

    let paused_animations = result
        .payload
        .get("paused_animations")
        .and_then(Value::as_u64);
    let web_animations_supported = result
        .payload
        .get("web_animations_supported")
        .and_then(Value::as_bool);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);
    let masked_elements = result
        .payload
        .get("masked_elements")
        .and_then(Value::as_u64);
    let mask_rects = sanitized_mask_rects(&result.payload);

    let valid_viewport = viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension);
    let valid_counts = paused_animations.is_some()
        && web_animations_supported.is_some()
        && masked_elements.is_some_and(|value| value <= MAX_MASKED_ELEMENTS)
        && mask_rects.is_some();

    if !valid_viewport || !valid_counts {
        fail_internal_capture_result(result, "visual_freeze_metadata_invalid");
        return;
    }

    let scroll_x = result.payload.get("scroll_x").and_then(Value::as_f64);
    let scroll_y = result.payload.get("scroll_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);
    let extension_present = result.payload.get("scroll_x").is_some()
        || result.payload.get("scroll_y").is_some()
        || result.payload.get("document_css_width").is_some()
        || result.payload.get("document_css_height").is_some();
    let extension_valid = scroll_x.is_some_and(valid_nonnegative_css_coordinate)
        && scroll_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height);

    if extension_present && !extension_valid {
        fail_internal_capture_result(result, "visual_freeze_metadata_invalid");
        return;
    }

    if extension_valid {
        result.payload = serde_json::json!({
            "paused_animations": paused_animations.expect("validated above"),
            "web_animations_supported": web_animations_supported.expect("validated above"),
            "viewport_css_width": viewport_css_width.expect("validated above"),
            "viewport_css_height": viewport_css_height.expect("validated above"),
            "masked_elements": masked_elements.expect("validated above"),
            "mask_rects": mask_rects.expect("validated above"),
            "scroll_x": scroll_x.expect("validated above"),
            "scroll_y": scroll_y.expect("validated above"),
            "document_css_width": document_css_width.expect("validated above"),
            "document_css_height": document_css_height.expect("validated above"),
        });
    } else {
        result.payload = serde_json::json!({
            "paused_animations": paused_animations.expect("validated above"),
            "web_animations_supported": web_animations_supported.expect("validated above"),
            "viewport_css_width": viewport_css_width.expect("validated above"),
            "viewport_css_height": viewport_css_height.expect("validated above"),
            "masked_elements": masked_elements.expect("validated above"),
            "mask_rects": mask_rects.expect("validated above"),
        });
    }
    result.error = None;
}

fn sanitize_capture_scroll_result(result: &mut BridgeActionResult) {
    if !result.ok {
        fail_internal_capture_result(result, "capture_scroll_failed");
        return;
    }

    let requested_y = result.payload.get("requested_y").and_then(Value::as_f64);
    let actual_x = result.payload.get("actual_x").and_then(Value::as_f64);
    let actual_y = result.payload.get("actual_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);

    let valid = requested_y.is_some_and(valid_full_page_y)
        && actual_x.is_some_and(valid_nonnegative_css_coordinate)
        && actual_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height)
        && viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension);

    if !valid {
        fail_internal_capture_result(result, "capture_scroll_metadata_invalid");
        return;
    }

    result.payload = serde_json::json!({
        "requested_y": requested_y.expect("validated above"),
        "actual_x": actual_x.expect("validated above"),
        "actual_y": actual_y.expect("validated above"),
        "document_css_width": document_css_width.expect("validated above"),
        "document_css_height": document_css_height.expect("validated above"),
        "viewport_css_width": viewport_css_width.expect("validated above"),
        "viewport_css_height": viewport_css_height.expect("validated above"),
    });
    result.error = None;
}

fn sanitize_capture_tile_probe_result(result: &mut BridgeActionResult) {
    if !result.ok {
        fail_internal_capture_result(result, "capture_tile_probe_failed");
        return;
    }

    let scroll_x = result.payload.get("scroll_x").and_then(Value::as_f64);
    let scroll_y = result.payload.get("scroll_y").and_then(Value::as_f64);
    let document_css_width = result
        .payload
        .get("document_css_width")
        .and_then(Value::as_f64);
    let document_css_height = result
        .payload
        .get("document_css_height")
        .and_then(Value::as_f64);
    let viewport_css_width = result
        .payload
        .get("viewport_css_width")
        .and_then(Value::as_f64);
    let viewport_css_height = result
        .payload
        .get("viewport_css_height")
        .and_then(Value::as_f64);
    let masked_elements = result
        .payload
        .get("masked_elements")
        .and_then(Value::as_u64);
    let mask_rects = sanitized_mask_rects(&result.payload);
    let positional_elements_scanned = result
        .payload
        .get("positional_elements_scanned")
        .and_then(Value::as_u64);
    let visible_fixed_or_sticky = result
        .payload
        .get("visible_fixed_or_sticky")
        .and_then(Value::as_bool);

    let valid = scroll_x.is_some_and(valid_nonnegative_css_coordinate)
        && scroll_y.is_some_and(valid_full_page_y)
        && document_css_width.is_some_and(valid_positive_css_dimension)
        && document_css_height.is_some_and(valid_positive_document_height)
        && viewport_css_width.is_some_and(valid_positive_css_dimension)
        && viewport_css_height.is_some_and(valid_positive_css_dimension)
        && masked_elements.is_some_and(|value| value <= MAX_MASKED_ELEMENTS)
        && mask_rects.is_some()
        && positional_elements_scanned.is_some_and(|value| value <= MAX_POSITIONAL_SCAN_ELEMENTS)
        && visible_fixed_or_sticky.is_some();

    if !valid {
        fail_internal_capture_result(result, "capture_tile_probe_metadata_invalid");
        return;
    }

    result.payload = serde_json::json!({
        "scroll_x": scroll_x.expect("validated above"),
        "scroll_y": scroll_y.expect("validated above"),
        "document_css_width": document_css_width.expect("validated above"),
        "document_css_height": document_css_height.expect("validated above"),
        "viewport_css_width": viewport_css_width.expect("validated above"),
        "viewport_css_height": viewport_css_height.expect("validated above"),
        "masked_elements": masked_elements.expect("validated above"),
        "mask_rects": mask_rects.expect("validated above"),
        "positional_elements_scanned": positional_elements_scanned.expect("validated above"),
        "visible_fixed_or_sticky": visible_fixed_or_sticky.expect("validated above"),
    });
    result.error = None;
}

fn fail_internal_capture_result(result: &mut BridgeActionResult, code: &'static str) {
    result.ok = false;
    result.payload = Value::Null;
    result.error = Some(code.into());
}

fn valid_positive_css_dimension(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_CSS_VIEWPORT_DIMENSION
}

fn valid_positive_document_height(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT
}

fn valid_nonnegative_css_coordinate(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_CSS_VIEWPORT_DIMENSION).contains(&value)
}

fn valid_full_page_y(value: f64) -> bool {
    value.is_finite() && (0.0..=MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT).contains(&value)
}

fn sanitized_mask_rects(payload: &Value) -> Option<Vec<Value>> {
    let rects = payload.get("mask_rects")?.as_array()?;
    if rects.len() > MAX_VISUAL_MASK_RECTS {
        return None;
    }

    let mut sanitized = Vec::with_capacity(rects.len());
    for rect in rects {
        let x = rect.get("x")?.as_f64()?;
        let y = rect.get("y")?.as_f64()?;
        let width = rect.get("width")?.as_f64()?;
        let height = rect.get("height")?.as_f64()?;
        let right = x + width;
        let bottom = y + height;
        if !x.is_finite()
            || !y.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || !right.is_finite()
            || !bottom.is_finite()
            || width <= 0.0
            || height <= 0.0
        {
            return None;
        }
        sanitized.push(serde_json::json!({
            "x": x,
            "y": y,
            "width": width,
            "height": height,
        }));
    }
    Some(sanitized)
}

impl Default for LiveBridge {
    fn default() -> Self {
        Self::new(2048, 128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64) -> ObserverEvent {
        ObserverEvent {
            seq,
            captured_at: Utc::now(),
            kind: ObserverEventKind::DomMutation,
            reference: None,
            route: None,
            payload: Value::Null,
        }
    }

    #[test]
    fn measure_result_storage_projects_geometry_and_strips_extra_fields() {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            reference: Some("@e1a2".into()),
            action: BridgeActionKind::Measure,
            created_at: Utc::now(),
        };
        let mut result = BridgeActionResult {
            action_id: action.id,
            ok: true,
            error: None,
            payload: serde_json::json!({
                "reference": "@e1a2",
                "rect": {"x": -2.5, "y": 20.0, "width": 100.0, "height": 40.0},
                "document_rect": {"x": -2.5, "y": 220.0, "width": 100.0, "height": 40.0},
                "viewport": {"width": 1280.0, "height": 720.0, "dpr": 2.0},
                "route": "http://127.0.0.1:5173/dashboard?secret=drop-me#fragment",
                "attributes": {"data-secret": "must-not-survive"},
                "name": "must-not-survive"
            }),
            completed_at: Utc::now(),
        };

        sanitize_result_for_storage(Some(&action), &mut result);
        assert!(result.ok);
        let text = result.payload.to_string();
        assert!(text.contains("@e1a2"));
        assert!(text.contains("document_rect"));
        assert!(!text.contains("must-not-survive"));
        assert!(!text.contains("data-secret"));
        assert!(!text.contains("dpr"));
        assert!(!text.contains("drop-me"));
        assert!(!text.contains("fragment"));
    }

    #[test]
    fn measure_result_storage_fails_closed_on_reference_or_route_mismatch() {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            reference: Some("@e1".into()),
            action: BridgeActionKind::Measure,
            created_at: Utc::now(),
        };

        for payload in [
            serde_json::json!({
                "reference": "@e2",
                "rect": {"x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0},
                "document_rect": {"x": 0.0, "y": 100.0, "width": 10.0, "height": 10.0},
                "viewport": {"width": 1280.0, "height": 720.0},
                "route": "http://127.0.0.1:5173/"
            }),
            serde_json::json!({
                "reference": "@e1",
                "rect": {"x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0},
                "document_rect": {"x": 0.0, "y": 100.0, "width": 10.0, "height": 10.0},
                "viewport": {"width": 1280.0, "height": 720.0},
                "route": "https://example.com/"
            }),
        ] {
            let mut result = BridgeActionResult {
                action_id: action.id,
                ok: true,
                error: Some("raw target error".into()),
                payload,
                completed_at: Utc::now(),
            };
            sanitize_result_for_storage(Some(&action), &mut result);
            assert!(!result.ok);
            assert_eq!(result.payload, Value::Null);
            assert_eq!(result.error.as_deref(), Some("measure action failed"));
        }
    }

    #[test]
    fn measure_failure_storage_drops_payload_and_raw_error() {
        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            reference: Some("@e1".into()),
            action: BridgeActionKind::Measure,
            created_at: Utc::now(),
        };
        let mut result = BridgeActionResult {
            action_id: action.id,
            ok: false,
            error: Some("secret target exception".into()),
            payload: serde_json::json!({"secret":"must-not-survive"}),
            completed_at: Utc::now(),
        };
        sanitize_result_for_storage(Some(&action), &mut result);
        assert_eq!(result.payload, Value::Null);
        assert_eq!(result.error.as_deref(), Some("measure action failed"));
    }

    #[test]
    fn measure_action_serializes_as_read_only_measure_type() {
        let encoded = serde_json::to_value(BridgeActionKind::Measure).unwrap();
        assert_eq!(encoded, serde_json::json!({"type": "measure"}));
        assert!(!BridgeActionKind::Measure.is_internal_capture_action());
    }

    #[tokio::test]
    async fn rejects_duplicate_sequences_and_resets_on_generation() {
        let bridge = LiveBridge::new(32, 8);
        let id = Uuid::new_v4();
        let first = bridge
            .ingest(ObserverBatch {
                session_id: id,
                generation: 1,
                events: vec![event(1), event(2), event(2)],
            })
            .await;
        assert_eq!(first.accepted, 2);
        assert_eq!(first.rejected_stale, 1);
        let second = bridge
            .ingest(ObserverBatch {
                session_id: id,
                generation: 2,
                events: vec![event(1)],
            })
            .await;
        assert_eq!(second.accepted, 1);
        assert_eq!(bridge.recent(id, 10).await.len(), 1);
    }

    #[tokio::test]
    async fn ingest_collect_returns_only_accepted_events() {
        let bridge = LiveBridge::new(32, 8);
        let id = Uuid::new_v4();
        let (report, accepted) = bridge
            .ingest_collect(ObserverBatch {
                session_id: id,
                generation: 1,
                events: vec![event(1), event(1), event(2)],
            })
            .await;
        assert_eq!(report.accepted, 2);
        assert_eq!(
            accepted.iter().map(|item| item.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[tokio::test]
    async fn action_origin_is_bounded_and_claimed_once() {
        let bridge = LiveBridge::new(32, 8);
        let id = Uuid::new_v4();
        let action = bridge
            .enqueue_action(
                id,
                Some("@e1".into()),
                BridgeActionKind::TypeText {
                    text: "private value".into(),
                    clear_first: true,
                },
            )
            .await;
        let taken = bridge.take_actions(id, 8).await;
        assert_eq!(taken.len(), 1);
        assert_eq!(
            bridge.claim_action(id, action.id).await.map(|item| item.id),
            Some(action.id)
        );
        assert!(bridge.claim_action(id, action.id).await.is_none());
    }

    #[tokio::test]
    async fn action_queue_is_bounded_and_drainable() {
        let bridge = LiveBridge::new(32, 8);
        let id = Uuid::new_v4();
        for _ in 0..10 {
            bridge
                .enqueue_action(id, None, BridgeActionKind::Click)
                .await;
        }
        assert_eq!(bridge.take_actions(id, 20).await.len(), 8);
    }

    #[tokio::test]
    async fn action_execution_boundary_is_daemon_owned_session_scoped_and_bounded() {
        let bridge = LiveBridge::new(32, 8);
        let session = Uuid::new_v4();
        let other = Uuid::new_v4();
        let client_completed_at = DateTime::<Utc>::from_timestamp(1, 0).expect("timestamp");

        let mut newest = None;
        for _ in 0..10 {
            let action = bridge
                .enqueue_action(session, None, BridgeActionKind::Snapshot)
                .await;
            assert!(bridge.action_execution_boundary(session, action.id).await.is_none());
            let taken = bridge.take_actions(session, 1).await;
            assert_eq!(taken.len(), 1);
            let claimed = bridge
                .claim_action(session, action.id)
                .await
                .expect("claimed action");
            bridge
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: action.id,
                        ok: true,
                        error: None,
                        payload: Value::Null,
                        completed_at: client_completed_at,
                    },
                )
                .await;
            let boundary = bridge
                .action_execution_boundary(session, action.id)
                .await
                .expect("daemon boundary");
            assert_eq!(boundary.action_id, action.id);
            assert_eq!(boundary.session_id, session);
            assert!(boundary.started_at <= boundary.completed_at);
            assert_ne!(boundary.completed_at, client_completed_at);
            assert!(
                bridge
                    .action_execution_boundary(other, action.id)
                    .await
                    .is_none()
            );
            newest = Some(action.id);
        }

        let boundaries = bridge.recent_action_execution_boundaries(session, 64).await;
        assert_eq!(boundaries.len(), 8, "boundary history follows result capacity");
        assert_eq!(boundaries.last().map(|item| item.action_id), newest);
    }

    #[tokio::test]
    async fn discarded_inflight_action_does_not_leave_an_execution_boundary() {
        let bridge = LiveBridge::new(32, 8);
        let session = Uuid::new_v4();
        let action = bridge
            .enqueue_action(session, None, BridgeActionKind::Focus)
            .await;
        assert_eq!(bridge.take_actions(session, 1).await.len(), 1);
        assert!(bridge.discard_public_action(session, action.id).await);
        assert!(
            bridge
                .action_execution_boundary(session, action.id)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn network_events_round_trip_through_bounded_history() {
        let bridge = LiveBridge::new(32, 8);
        let id = Uuid::new_v4();
        bridge
            .ingest(ObserverBatch {
                session_id: id,
                generation: 1,
                events: vec![ObserverEvent {
                    seq: 1,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Network,
                    reference: None,
                    route: Some("/".into()),
                    payload: serde_json::json!({"method":"GET","status":200}),
                }],
            })
            .await;
        assert_eq!(bridge.recent(id, 10).await.len(), 1);
    }
}
