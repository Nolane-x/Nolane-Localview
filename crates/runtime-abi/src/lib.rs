#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion { pub major: u16, pub minor: u16 }

impl ProtocolVersion {
    pub const V2: Self = Self { major: 2, minor: 0 };
    pub fn compatible_with(self, other: Self) -> bool { self.major == other.major }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind { Project, Session, Route, Region, Element, Source, Request, Service, Evidence, Proof, Candidate, Contract, Persona }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityId { pub kind: EntityKind, pub namespace: String, pub key: String }

impl EntityId {
    pub fn canonical(&self) -> String { format!("{:?}:{}:{}", self.kind, self.namespace, self.key) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEntity {
    pub id: EntityId,
    pub revision: u64,
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AbiCapability {
    Observe,
    SemanticSnapshot,
    VisualCapture,
    Interact,
    Replay,
    Network,
    Console,
    Accessibility,
    Performance,
    SourceMap,
    FailureInjection,
    Evidence,
    Causal,
    Contracts,
    Counterfactual,
    Mutation,
    Attestation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilitySet {
    pub protocol: ProtocolVersion,
    pub supported: BTreeSet<AbiCapability>,
    pub required: BTreeSet<AbiCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NegotiationResult {
    pub compatible: bool,
    pub agreed: BTreeSet<AbiCapability>,
    pub missing_required: BTreeSet<AbiCapability>,
    pub reason: Option<String>,
}

pub fn negotiate(local: &CapabilitySet, remote: &CapabilitySet) -> NegotiationResult {
    if !local.protocol.compatible_with(remote.protocol) {
        return NegotiationResult { compatible: false, agreed: BTreeSet::new(), missing_required: BTreeSet::new(), reason: Some("protocol major version mismatch".into()) };
    }
    let agreed = local.supported.intersection(&remote.supported).copied().collect::<BTreeSet<_>>();
    let required = local.required.union(&remote.required).copied().collect::<BTreeSet<_>>();
    let missing_required = required.difference(&agreed).copied().collect::<BTreeSet<_>>();
    NegotiationResult { compatible: missing_required.is_empty(), agreed, missing_required: missing_required.clone(), reason: (!missing_required.is_empty()).then_some("required runtime capabilities are unavailable".into()) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEvent {
    pub id: Uuid,
    pub sequence: u64,
    pub stream: String,
    pub entity: Option<EntityId>,
    pub event_type: String,
    pub payload: Value,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Subscription {
    pub stream_prefixes: Vec<String>,
    pub entity_kinds: BTreeSet<EntityKind>,
    pub event_types: BTreeSet<String>,
    pub max_buffered: usize,
}

impl Subscription {
    pub fn matches(&self, event: &RuntimeEvent) -> bool {
        let stream_match = self.stream_prefixes.is_empty() || self.stream_prefixes.iter().any(|prefix| event.stream.starts_with(prefix));
        let entity_match = self.entity_kinds.is_empty() || event.entity.as_ref().is_some_and(|entity| self.entity_kinds.contains(&entity.kind));
        let event_match = self.event_types.is_empty() || self.event_types.contains(&event.event_type);
        stream_match && entity_match && event_match
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackpressurePolicy { pub max_buffered: usize, pub drop_oldest_observational: bool, pub never_drop_types: BTreeSet<String> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventBuffer {
    pub events: Vec<RuntimeEvent>,
    pub dropped: usize,
}

impl EventBuffer {
    pub fn push(&mut self, event: RuntimeEvent, policy: &BackpressurePolicy) -> bool {
        let capacity = policy.max_buffered.max(1);
        if self.events.len() < capacity { self.events.push(event); return true; }
        if policy.never_drop_types.contains(&event.event_type) {
            if let Some(index) = self.events.iter().position(|existing| !policy.never_drop_types.contains(&existing.event_type)) {
                self.events.remove(index);
                self.dropped += 1;
                self.events.push(event);
                return true;
            }
            return false;
        }
        if policy.drop_oldest_observational {
            self.events.remove(0);
            self.dropped += 1;
            self.events.push(event);
            true
        } else {
            self.dropped += 1;
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaDescriptor { pub name: String, pub version: u32, pub compatible_from: u32 }

impl SchemaDescriptor {
    pub fn can_read(&self, stored_version: u32) -> bool { stored_version >= self.compatible_from && stored_version <= self.version }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_rejects_missing_required_capability() {
        let local = CapabilitySet { protocol: ProtocolVersion::V2, supported: BTreeSet::from([AbiCapability::Observe]), required: BTreeSet::from([AbiCapability::Observe]) };
        let remote = CapabilitySet { protocol: ProtocolVersion::V2, supported: BTreeSet::from([AbiCapability::VisualCapture]), required: BTreeSet::new() };
        assert!(!negotiate(&local, &remote).compatible);
    }

    #[test]
    fn backpressure_preserves_critical_event_when_possible() {
        let policy = BackpressurePolicy { max_buffered: 1, drop_oldest_observational: true, never_drop_types: BTreeSet::from(["proof".into()]) };
        let mut buffer = EventBuffer { events: Vec::new(), dropped: 0 };
        let event = |kind: &str| RuntimeEvent { id: Uuid::new_v4(), sequence: 1, stream: "test".into(), entity: None, event_type: kind.into(), payload: Value::Null, evidence_ids: vec![] };
        buffer.push(event("scroll"), &policy);
        assert!(buffer.push(event("proof"), &policy));
        assert_eq!(buffer.events[0].event_type, "proof");
    }
}