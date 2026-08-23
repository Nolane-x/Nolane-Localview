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

#[derive(Debug, Default)]
struct SessionBridgeState {
    generation: u64,
    last_seq: Option<u64>,
    events: VecDeque<ObserverEvent>,
    actions: VecDeque<BridgeAction>,
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
        let mut states = self.inner.write().await;
        let Some(state) = states.get_mut(&session_id) else {
            return Vec::new();
        };
        let count = limit.min(state.actions.len());
        state.actions.drain(..count).collect()
    }

    pub async fn complete_action(&self, session_id: SessionId, result: BridgeActionResult) {
        let mut states = self.inner.write().await;
        let state = states.entry(session_id).or_default();
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
        assert_eq!(accepted.iter().map(|item| item.seq).collect::<Vec<_>>(), vec![1, 2]);
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
