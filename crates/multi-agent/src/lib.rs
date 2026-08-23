#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type AgentId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentIdentity {
    pub id: AgentId,
    pub name: String,
    pub principal: String,
    pub capabilities: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel { ReadOnly, SharedPerception, IsolatedMutation, ExclusiveMutation }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentLease {
    pub id: Uuid,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub isolation: IsolationLevel,
    pub resources: BTreeSet<String>,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl AgentLease {
    pub fn active_at(&self, now: DateTime<Utc>) -> bool { now < self.expires_at }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionIntent { Observe, Interact, Test, MutateSource, ApplyCandidate, ExternalSideEffect }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub id: Uuid,
    pub agent_id: AgentId,
    pub session_id: SessionId,
    pub intent: ActionIntent,
    pub target: Option<String>,
    pub base_revision: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Conflict {
    pub left_lease: Uuid,
    pub right_lease: Uuid,
    pub resources: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct LeaseTable {
    leases: BTreeMap<Uuid, AgentLease>,
}

impl LeaseTable {
    pub fn acquire(
        &mut self,
        agent_id: AgentId,
        session_id: SessionId,
        isolation: IsolationLevel,
        resources: BTreeSet<String>,
        ttl: Duration,
        now: DateTime<Utc>,
    ) -> Result<AgentLease, Vec<Conflict>> {
        self.reap(now);
        let proposed = AgentLease { id: Uuid::new_v4(), agent_id, session_id, isolation, resources, acquired_at: now, expires_at: now + ttl.max(Duration::seconds(1)) };
        let conflicts = self.leases.values().filter(|existing| existing.session_id == session_id).filter_map(|existing| conflict(existing, &proposed)).collect::<Vec<_>>();
        if !conflicts.is_empty() { return Err(conflicts); }
        self.leases.insert(proposed.id, proposed.clone());
        Ok(proposed)
    }

    pub fn renew(&mut self, lease_id: Uuid, ttl: Duration, now: DateTime<Utc>) -> bool {
        let Some(lease) = self.leases.get_mut(&lease_id) else { return false; };
        if !lease.active_at(now) { self.leases.remove(&lease_id); return false; }
        lease.expires_at = now + ttl.max(Duration::seconds(1));
        true
    }

    pub fn release(&mut self, lease_id: Uuid) -> bool { self.leases.remove(&lease_id).is_some() }

    pub fn reap(&mut self, now: DateTime<Utc>) { self.leases.retain(|_, lease| lease.active_at(now)); }

    pub fn active_for_session(&self, session_id: SessionId, now: DateTime<Utc>) -> Vec<&AgentLease> {
        self.leases.values().filter(|lease| lease.session_id == session_id && lease.active_at(now)).collect()
    }
}

fn conflict(left: &AgentLease, right: &AgentLease) -> Option<Conflict> {
    if left.agent_id == right.agent_id { return None; }
    let overlap = left.resources.intersection(&right.resources).cloned().collect::<Vec<_>>();
    if overlap.is_empty() { return None; }
    let mutation_conflict = left.isolation >= IsolationLevel::IsolatedMutation || right.isolation >= IsolationLevel::IsolatedMutation;
    if !mutation_conflict { return None; }
    Some(Conflict { left_lease: left.id, right_lease: right.id, resources: overlap, reason: "overlapping mutable resource lease".into() })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalConflict {
    pub left_entry: Uuid,
    pub right_entry: Uuid,
    pub target: String,
    pub reason: String,
}

pub fn detect_journal_conflicts(entries: &[JournalEntry]) -> Vec<JournalConflict> {
    let mut conflicts = Vec::new();
    for (index, left) in entries.iter().enumerate() {
        for right in entries.iter().skip(index + 1) {
            if left.agent_id == right.agent_id || left.session_id != right.session_id { continue; }
            let Some(target) = left.target.as_ref().filter(|target| right.target.as_ref() == Some(*target)) else { continue; };
            let both_mutate = matches!(left.intent, ActionIntent::MutateSource | ActionIntent::ApplyCandidate)
                && matches!(right.intent, ActionIntent::MutateSource | ActionIntent::ApplyCandidate);
            if both_mutate && left.base_revision == right.base_revision {
                conflicts.push(JournalConflict { left_entry: left.id, right_entry: right.id, target: target.clone(), reason: "concurrent mutations share the same base revision".into() });
            }
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_perception_does_not_block_another_reader() {
        let now = Utc::now();
        let session = Uuid::new_v4();
        let mut table = LeaseTable::default();
        table.acquire(Uuid::new_v4(), session, IsolationLevel::SharedPerception, BTreeSet::from(["hero".into()]), Duration::seconds(30), now).expect("first lease");
        assert!(table.acquire(Uuid::new_v4(), session, IsolationLevel::SharedPerception, BTreeSet::from(["hero".into()]), Duration::seconds(30), now).is_ok());
    }

    #[test]
    fn exclusive_mutation_blocks_overlapping_mutation() {
        let now = Utc::now();
        let session = Uuid::new_v4();
        let mut table = LeaseTable::default();
        table.acquire(Uuid::new_v4(), session, IsolationLevel::ExclusiveMutation, BTreeSet::from(["src/App.tsx".into()]), Duration::seconds(30), now).expect("first lease");
        assert!(table.acquire(Uuid::new_v4(), session, IsolationLevel::IsolatedMutation, BTreeSet::from(["src/App.tsx".into()]), Duration::seconds(30), now).is_err());
    }
}