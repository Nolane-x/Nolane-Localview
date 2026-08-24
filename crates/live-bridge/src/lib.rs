#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::{ElementRef, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

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
    FreezeVisuals,
    RestoreVisuals { token: Uuid },
}

impl BridgeActionKind {
    pub fn is_internal_capture_action(&self) -> bool {
        matches!(
            self,
            Self::FreezeVisuals | Self::RestoreVisuals { .. }
        )
    }
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
pub struct BridgeActionResult {
    pub action_id: Uuid,
    pub ok: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub completed_at: DateTime<Utc>,
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

#[derive(Debug, Default)]
struct SessionBridgeState {
    generation: u64,
    last_seq: Option<u64>,
    events: VecDeque<ObserverEvent>,
    actions: VecDeque<BridgeAction>,
    inflight: VecDeque<BridgeAction>,
    claimed: VecDeque<BridgeAction>,
    results: VecDeque<BridgeActionResult>,
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
            .map(|state| {
                state
                    .events
                    .iter()
                    .rev()
                    .take(limit)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            })
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
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
        state.actions.push_back(action.clone());
        while state.actions.len() > self.action_capacity {
            state.actions.pop_front();
        }
        action
    }

    pub async fn take_actions(&self, session_id: SessionId, limit: usize) -> Vec<BridgeAction> {
        self.take_public_actions(session_id, limit).await
    }

    pub async fn take_public_actions(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeAction> {
        self.take_actions_by_scope(session_id, limit, false).await
    }

    pub async fn take_internal_capture_actions(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeAction> {
        self.take_actions_by_scope(session_id, limit, true).await
    }

    async fn take_actions_by_scope(
        &self,
        session_id: SessionId,
        limit: usize,
        internal_capture: bool,
    ) -> Vec<BridgeAction> {
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };

        let mut selected = Vec::with_capacity(limit.min(state.actions.len()));
        let mut remaining = VecDeque::with_capacity(state.actions.len());
        while let Some(action) = state.actions.pop_front() {
            let scope_matches = action.action.is_internal_capture_action() == internal_capture;
            if scope_matches && selected.len() < limit {
                selected.push(action);
            } else {
                remaining.push_back(action);
            }
        }
        state.actions = remaining;
        move_to_inflight(state, &selected, self.action_capacity);
        selected
    }

    pub async fn claim_action(
        &self,
        session_id: SessionId,
        action_id: Uuid,
    ) -> Option<BridgeAction> {
        let mut states = self.inner.write().await;
        let state = states.get_mut(&session_id)?;
        let index = state
            .inflight
            .iter()
            .position(|action| action.id == action_id)?;
        let action = state.inflight.remove(index)?;
        state.claimed.push_back(action.clone());
        while state.claimed.len() > self.action_capacity {
            state.claimed.pop_front();
        }
        Some(action)
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

        let action = match origin {
            CompletionOrigin::Action(action) => {
                if let Some(index) = state
                    .claimed
                    .iter()
                    .position(|claimed| claimed.id == action.id)
                {
                    state.claimed.remove(index);
                }
                Some(action)
            }
            CompletionOrigin::Session(_) => state
                .claimed
                .iter()
                .position(|claimed| claimed.id == result.action_id)
                .and_then(|index| state.claimed.remove(index)),
        };

        sanitize_result_for_storage(action.as_ref(), &mut result);
        state.results.push_back(result);
        while state.results.len() > self.result_capacity {
            state.results.pop_front();
        }
    }

    pub async fn recent_results(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<BridgeActionResult> {
        let states = self.inner.read().await;
        states
            .get(&session_id)
            .map(|state| {
                state
                    .results
                    .iter()
                    .rev()
                    .take(limit)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn release_session(&self, session_id: SessionId) {
        self.inner.write().await.remove(&session_id);
    }
}

fn move_to_inflight(
    state: &mut SessionBridgeState,
    actions: &[BridgeAction],
    action_capacity: usize,
) {
    for action in actions {
        state.inflight.push_back(action.clone());
    }
    while state.inflight.len() > action_capacity {
        state.inflight.pop_front();
    }
}

fn sanitize_result_for_storage(action: Option<&BridgeAction>, result: &mut BridgeActionResult) {
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
        Some(BridgeActionKind::FreezeVisuals) => {
            let paused_animations = result
                .payload
                .get("paused_animations")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let web_animations_supported = result
                .payload
                .get("web_animations_supported")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            result.payload = serde_json::json!({
                "paused_animations": paused_animations,
                "web_animations_supported": web_animations_supported,
            });
            if result.error.is_some() {
                result.error = Some("visual freeze action failed".into());
            }
        }
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
