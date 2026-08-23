#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use chrono::{DateTime, Utc};
use localview_content_addressed::ObjectHash;
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeCheckpoint {
    pub id: Uuid,
    pub session_id: SessionId,
    pub revision: String,
    pub environment_hash: String,
    pub route: String,
    pub state_root: ObjectHash,
    pub evidence_roots: Vec<ObjectHash>,
    pub created_at: DateTime<Utc>,
    pub restorable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestoreRequest {
    pub checkpoint_id: Uuid,
    pub current_revision: String,
    pub current_environment_hash: String,
    pub allow_revision_mismatch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RestoreDecision {
    Allowed,
    Denied { reasons: Vec<String> },
}

pub fn can_restore(checkpoint: &RuntimeCheckpoint, request: &RestoreRequest) -> RestoreDecision {
    let mut reasons = Vec::new();
    if !checkpoint.restorable { reasons.push("checkpoint was recorded as observation-only".into()); }
    if checkpoint.environment_hash != request.current_environment_hash { reasons.push("environment hash differs".into()); }
    if checkpoint.revision != request.current_revision && !request.allow_revision_mismatch { reasons.push("source revision differs".into()); }
    if reasons.is_empty() { RestoreDecision::Allowed } else { RestoreDecision::Denied { reasons } }
}

#[derive(Debug, Clone)]
pub struct CheckpointStore {
    capacity_per_session: usize,
    by_session: BTreeMap<SessionId, VecDeque<RuntimeCheckpoint>>,
}

impl CheckpointStore {
    pub fn new(capacity_per_session: usize) -> Self { Self { capacity_per_session: capacity_per_session.max(2), by_session: BTreeMap::new() } }

    pub fn push(&mut self, checkpoint: RuntimeCheckpoint) {
        let queue = self.by_session.entry(checkpoint.session_id).or_default();
        queue.push_back(checkpoint);
        while queue.len() > self.capacity_per_session { queue.pop_front(); }
    }

    pub fn latest(&self, session_id: SessionId) -> Option<&RuntimeCheckpoint> { self.by_session.get(&session_id).and_then(|queue| queue.back()) }

    pub fn get(&self, session_id: SessionId, checkpoint_id: Uuid) -> Option<&RuntimeCheckpoint> {
        self.by_session.get(&session_id)?.iter().find(|checkpoint| checkpoint.id == checkpoint_id)
    }

    pub fn release_session(&mut self, session_id: SessionId) { self.by_session.remove(&session_id); }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheKey {
    pub session_id: SessionId,
    pub revision: String,
    pub environment_hash: String,
    pub route: String,
    pub viewport: String,
    pub region: String,
}

impl CacheKey {
    pub fn stable_key(&self) -> String { format!("{}|{}|{}|{}|{}|{}", self.session_id, self.revision, self.environment_hash, self.route, self.viewport, self.region) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptualCacheEntry {
    pub key: CacheKey,
    pub object_hashes: Vec<ObjectHash>,
    pub source_dependencies: BTreeSet<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct PerceptualCache {
    entries: BTreeMap<String, PerceptualCacheEntry>,
}

impl PerceptualCache {
    pub fn put(&mut self, entry: PerceptualCacheEntry) { self.entries.insert(entry.key.stable_key(), entry); }

    pub fn get(&self, key: &CacheKey) -> Option<&PerceptualCacheEntry> { self.entries.get(&key.stable_key()) }

    pub fn invalidate_changed_sources(&mut self, changed_files: &BTreeSet<String>) -> Vec<String> {
        let invalid = self.entries.iter().filter(|(_, entry)| !entry.source_dependencies.is_disjoint(changed_files)).map(|(key, _)| key.clone()).collect::<Vec<_>>();
        for key in &invalid { self.entries.remove(key); }
        invalid
    }

    pub fn invalidate_revision(&mut self, revision: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, entry| entry.key.revision == revision);
        before.saturating_sub(self.entries.len())
    }

    pub fn release_session(&mut self, session_id: SessionId) { self.entries.retain(|_, entry| entry.key.session_id != session_id); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_rejects_environment_drift() {
        let session = Uuid::new_v4();
        let checkpoint = RuntimeCheckpoint { id: Uuid::new_v4(), session_id: session, revision: "a".into(), environment_hash: "env-a".into(), route: "/".into(), state_root: "state".into(), evidence_roots: vec![], created_at: Utc::now(), restorable: true };
        let decision = can_restore(&checkpoint, &RestoreRequest { checkpoint_id: checkpoint.id, current_revision: "a".into(), current_environment_hash: "env-b".into(), allow_revision_mismatch: false });
        assert!(matches!(decision, RestoreDecision::Denied { .. }));
    }

    #[test]
    fn cache_invalidates_only_dependent_regions() {
        let session = Uuid::new_v4();
        let mut cache = PerceptualCache::default();
        let make = |region: &str, source: &str| PerceptualCacheEntry { key: CacheKey { session_id: session, revision: "a".into(), environment_hash: "env".into(), route: "/".into(), viewport: "desktop".into(), region: region.into() }, object_hashes: vec![], source_dependencies: BTreeSet::from([source.into()]), created_at: Utc::now() };
        cache.put(make("hero", "Hero.tsx"));
        cache.put(make("footer", "Footer.tsx"));
        let invalid = cache.invalidate_changed_sources(&BTreeSet::from(["Hero.tsx".into()]));
        assert_eq!(invalid.len(), 1);
        assert!(cache.get(&make("footer", "Footer.tsx").key).is_some());
    }
}