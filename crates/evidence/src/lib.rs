#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

pub type EvidenceId = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Visual,
    Semantic,
    Layout,
    Console,
    Network,
    Source,
    Interaction,
    Performance,
    Accessibility,
    Contract,
    Test,
    Causal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyClass {
    Observed,
    Derived,
    Heuristic,
    Subjective,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Provenance {
    pub source: String,
    pub engine: Option<String>,
    pub revision: Option<String>,
    pub parent_ids: Vec<EvidenceId>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceObject {
    pub id: EvidenceId,
    pub kind: EvidenceKind,
    pub session_id: SessionId,
    pub region: Option<String>,
    pub payload: Value,
    pub provenance: Provenance,
    pub confidence: f32,
    pub uncertainty: UncertaintyClass,
    pub secret_taint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceDraft {
    pub kind: EvidenceKind,
    pub session_id: SessionId,
    pub region: Option<String>,
    pub payload: Value,
    pub provenance: Provenance,
    pub confidence: f32,
    pub uncertainty: UncertaintyClass,
    pub secret_taint: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InsertReport {
    pub id: EvidenceId,
    pub deduplicated: bool,
}

#[derive(Debug, Default)]
struct StoreState {
    by_id: HashMap<EvidenceId, EvidenceObject>,
    order: VecDeque<EvidenceId>,
}

#[derive(Debug, Clone)]
pub struct EvidenceStore {
    state: Arc<RwLock<StoreState>>,
    capacity: usize,
}

impl EvidenceStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Arc::new(RwLock::new(StoreState::default())),
            capacity: capacity.max(64),
        }
    }

    pub async fn insert(&self, draft: EvidenceDraft) -> InsertReport {
        let id = evidence_id(&draft);
        let mut state = self.state.write().await;
        if state.by_id.contains_key(&id) {
            return InsertReport { id, deduplicated: true };
        }
        let object = EvidenceObject {
            id: id.clone(),
            kind: draft.kind,
            session_id: draft.session_id,
            region: draft.region,
            payload: draft.payload,
            provenance: draft.provenance,
            confidence: draft.confidence.clamp(0.0, 1.0),
            uncertainty: draft.uncertainty,
            secret_taint: draft.secret_taint,
        };
        state.order.push_back(id.clone());
        state.by_id.insert(id.clone(), object);
        while state.order.len() > self.capacity {
            if let Some(oldest) = state.order.pop_front() {
                state.by_id.remove(&oldest);
            }
        }
        InsertReport { id, deduplicated: false }
    }

    pub async fn get(&self, id: &str) -> Option<EvidenceObject> {
        self.state.read().await.by_id.get(id).cloned()
    }

    pub async fn recent_for_session(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<EvidenceObject> {
        let state = self.state.read().await;
        state
            .order
            .iter()
            .rev()
            .filter_map(|id| state.by_id.get(id))
            .filter(|evidence| evidence.session_id == session_id)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub async fn mark_revision_stale(&self, revision: &str) -> Vec<EvidenceId> {
        let state = self.state.read().await;
        state
            .by_id
            .values()
            .filter(|evidence| {
                evidence
                    .provenance
                    .revision
                    .as_deref()
                    .is_some_and(|value| value != revision)
            })
            .map(|evidence| evidence.id.clone())
            .collect()
    }

    pub async fn release_session(&self, session_id: SessionId) {
        let mut state = self.state.write().await;
        state.by_id.retain(|_, evidence| evidence.session_id != session_id);
        let remaining = state.by_id.keys().cloned().collect::<HashSet<_>>();
        state.order.retain(|id| remaining.contains(id));
    }
}

impl Default for EvidenceStore {
    fn default() -> Self { Self::new(4096) }
}

pub fn evidence_id(draft: &EvidenceDraft) -> EvidenceId {
    let canonical = canonical_value(serde_json::to_value(draft).unwrap_or(Value::Null));
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    format!("ev_{}", hex::encode(digest))
}

fn canonical_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .into_iter()
                .map(|(key, value)| (key, canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn draft(session_id: SessionId) -> EvidenceDraft {
        EvidenceDraft {
            kind: EvidenceKind::Layout,
            session_id,
            region: Some("hero".into()),
            payload: serde_json::json!({"b": 2, "a": 1}),
            provenance: Provenance {
                source: "layout-engine".into(),
                engine: Some("native".into()),
                revision: Some("abc".into()),
                parent_ids: Vec::new(),
                captured_at: DateTime::<Utc>::from_timestamp(1, 0).expect("timestamp"),
            },
            confidence: 0.95,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        }
    }

    #[tokio::test]
    async fn identical_evidence_is_deduplicated() {
        let store = EvidenceStore::new(64);
        let session = Uuid::new_v4();
        let first = store.insert(draft(session)).await;
        let second = store.insert(draft(session)).await;
        assert_eq!(first.id, second.id);
        assert!(!first.deduplicated);
        assert!(second.deduplicated);
    }

    #[tokio::test]
    async fn releasing_session_removes_only_its_evidence() {
        let store = EvidenceStore::new(64);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        store.insert(draft(first)).await;
        store.insert(draft(second)).await;
        store.release_session(first).await;
        assert!(store.recent_for_session(first, 10).await.is_empty());
        assert_eq!(store.recent_for_session(second, 10).await.len(), 1);
    }
}
