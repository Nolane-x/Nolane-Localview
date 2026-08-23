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
    Coverage,
    Proof,
    Repro,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RetentionTier {
    Ephemeral,
    Session,
    Project,
    Baseline,
    Pinned,
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
    retention: HashMap<EvidenceId, RetentionTier>,
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
        self.insert_with_retention(draft, RetentionTier::Session)
            .await
    }

    pub async fn insert_with_retention(
        &self,
        draft: EvidenceDraft,
        retention: RetentionTier,
    ) -> InsertReport {
        let id = evidence_id(&draft);
        let mut state = self.state.write().await;
        if state.by_id.contains_key(&id) {
            let current = state
                .retention
                .entry(id.clone())
                .or_insert(RetentionTier::Session);
            if retention > *current {
                *current = retention;
            }
            return InsertReport {
                id,
                deduplicated: true,
            };
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
        state.retention.insert(id.clone(), retention);
        state.by_id.insert(id.clone(), object);
        while state.by_id.len() > self.capacity {
            let candidate = state.order.iter().position(|candidate_id| {
                state
                    .retention
                    .get(candidate_id)
                    .copied()
                    .unwrap_or(RetentionTier::Session)
                    < RetentionTier::Baseline
            });
            let Some(index) = candidate else {
                break;
            };
            if let Some(oldest) = state.order.remove(index) {
                state.by_id.remove(&oldest);
                state.retention.remove(&oldest);
            }
        }
        InsertReport {
            id,
            deduplicated: false,
        }
    }

    pub async fn get(&self, id: &str) -> Option<EvidenceObject> {
        self.state.read().await.by_id.get(id).cloned()
    }

    pub async fn retention(&self, id: &str) -> Option<RetentionTier> {
        self.state.read().await.retention.get(id).copied()
    }

    pub async fn set_retention(&self, id: &str, retention: RetentionTier) -> bool {
        let mut state = self.state.write().await;
        if !state.by_id.contains_key(id) {
            return false;
        }
        state.retention.insert(id.to_owned(), retention);
        true
    }

    pub async fn trace(&self, id: &str, max_depth: usize) -> Vec<EvidenceObject> {
        let state = self.state.read().await;
        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([(id.to_owned(), 0usize)]);
        let mut trace = Vec::new();
        while let Some((current, depth)) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            let Some(evidence) = state.by_id.get(&current) else {
                continue;
            };
            trace.push(evidence.clone());
            if depth >= max_depth {
                continue;
            }
            for parent in &evidence.provenance.parent_ids {
                queue.push_back((parent.clone(), depth + 1));
            }
        }
        trace
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
        let keep = state
            .by_id
            .iter()
            .filter(|(id, evidence)| {
                evidence.session_id != session_id
                    || state
                        .retention
                        .get(*id)
                        .copied()
                        .unwrap_or(RetentionTier::Session)
                        >= RetentionTier::Project
            })
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        state.by_id.retain(|id, _| keep.contains(id));
        state.order.retain(|id| keep.contains(id));
        state.retention.retain(|id, _| keep.contains(id));
    }
}

impl Default for EvidenceStore {
    fn default() -> Self {
        Self::new(4096)
    }
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
    async fn identical_evidence_is_deduplicated_and_can_raise_retention() {
        let store = EvidenceStore::new(64);
        let session = Uuid::new_v4();
        let first = store.insert(draft(session)).await;
        let second = store
            .insert_with_retention(draft(session), RetentionTier::Pinned)
            .await;
        assert_eq!(first.id, second.id);
        assert!(!first.deduplicated);
        assert!(second.deduplicated);
        assert_eq!(
            store.retention(&first.id).await,
            Some(RetentionTier::Pinned)
        );
    }

    #[tokio::test]
    async fn trace_follows_parent_provenance_without_cycles() {
        let store = EvidenceStore::new(64);
        let session = Uuid::new_v4();
        let parent = store.insert(draft(session)).await.id;
        let mut child = draft(session);
        child.payload = serde_json::json!({"child": true});
        child.provenance.parent_ids = vec![parent.clone()];
        let child = store.insert(child).await.id;
        let trace = store.trace(&child, 4).await;
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].id, child);
        assert_eq!(trace[1].id, parent);
    }

    #[tokio::test]
    async fn releasing_session_preserves_project_retention_only() {
        let store = EvidenceStore::new(64);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let session_only = store.insert(draft(first)).await.id;
        let project = store
            .insert_with_retention(draft(second), RetentionTier::Project)
            .await
            .id;
        store.release_session(first).await;
        store.release_session(second).await;
        assert!(store.get(&session_only).await.is_none());
        assert!(store.get(&project).await.is_some());
    }
}
