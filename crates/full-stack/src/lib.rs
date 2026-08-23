#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use localview_protocol::SourceLocation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind { Frontend, Api, Worker, Database, Cache, Queue, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalService {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    pub address: Option<String>,
    pub process_id: Option<u32>,
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceEdge {
    pub from: String,
    pub to: String,
    pub protocol: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceGraph {
    pub services: BTreeMap<String, LocalService>,
    pub edges: Vec<ServiceEdge>,
}

impl ServiceGraph {
    pub fn add_service(&mut self, service: LocalService) { self.services.insert(service.id.clone(), service); }

    pub fn add_edge(&mut self, edge: ServiceEdge) -> bool {
        if !self.services.contains_key(&edge.from) || !self.services.contains_key(&edge.to) { return false; }
        if !self.edges.contains(&edge) { self.edges.push(edge); }
        true
    }

    pub fn reachable_from(&self, start: &str, max_depth: usize) -> Vec<String> {
        if !self.services.contains_key(start) { return Vec::new(); }
        let mut queue = VecDeque::from([(start.to_owned(), 0usize)]);
        let mut visited = BTreeSet::from([start.to_owned()]);
        let mut result = Vec::new();
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for edge in self.edges.iter().filter(|edge| edge.from == current) {
                if visited.insert(edge.to.clone()) {
                    result.push(edge.to.clone());
                    queue.push_back((edge.to.clone(), depth + 1));
                }
            }
        }
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestTrace {
    pub request_id: String,
    pub method: String,
    pub url: String,
    pub frontend_source: Option<SourceLocation>,
    pub backend_source: Option<SourceLocation>,
    pub service_path: Vec<String>,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorChainNode {
    pub layer: String,
    pub message: String,
    pub source: Option<SourceLocation>,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FullStackErrorChain { pub nodes: Vec<ErrorChainNode> }

impl FullStackErrorChain {
    pub fn push(&mut self, node: ErrorChainNode) {
        if self.nodes.last() != Some(&node) { self.nodes.push(node); }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SchemaShape {
    pub name: String,
    pub fields: BTreeMap<String, String>,
    pub required: BTreeSet<String>,
    pub examples: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaDrift {
    pub added_fields: Vec<String>,
    pub removed_fields: Vec<String>,
    pub changed_types: Vec<(String, String, String)>,
    pub newly_required: Vec<String>,
}

pub fn diff_schema(before: &SchemaShape, after: &SchemaShape) -> SchemaDrift {
    let before_keys = before.fields.keys().cloned().collect::<BTreeSet<_>>();
    let after_keys = after.fields.keys().cloned().collect::<BTreeSet<_>>();
    let changed_types = before_keys.intersection(&after_keys).filter_map(|field| {
        let old = before.fields.get(field)?;
        let new = after.fields.get(field)?;
        (old != new).then_some((field.clone(), old.clone(), new.clone()))
    }).collect();
    SchemaDrift {
        added_fields: after_keys.difference(&before_keys).cloned().collect(),
        removed_fields: before_keys.difference(&after_keys).cloned().collect(),
        changed_types,
        newly_required: after.required.difference(&before.required).cloned().collect(),
    }
}

impl SchemaDrift {
    pub fn breaking(&self) -> bool { !self.removed_fields.is_empty() || !self.changed_types.is_empty() || !self.newly_required.is_empty() }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AsyncFlowNode {
    pub id: String,
    pub kind: String,
    pub queue: Option<String>,
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AsyncFlowEdge { pub from: String, pub to: String, pub correlation_key: Option<String> }

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AsyncFlowGraph { pub nodes: BTreeMap<String, AsyncFlowNode>, pub edges: Vec<AsyncFlowEdge> }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseBoundary {
    pub service_id: String,
    pub database_kind: String,
    pub operation: String,
    pub table_or_collection: Option<String>,
    pub source: Option<SourceLocation>,
    pub mutating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FullStackCheckpoint {
    pub revision: String,
    pub service_ids: Vec<String>,
    pub schema_hashes: BTreeMap<String, String>,
    pub pending_async_flows: Vec<String>,
    pub database_mutations_blocked: bool,
}

pub fn safe_checkpoint(checkpoint: &FullStackCheckpoint) -> bool {
    checkpoint.database_mutations_blocked && checkpoint.pending_async_flows.is_empty() && !checkpoint.revision.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_drift_marks_new_required_field_as_breaking() {
        let before = SchemaShape { name: "User".into(), fields: BTreeMap::from([("id".into(), "string".into())]), required: BTreeSet::from(["id".into()]), examples: BTreeMap::new() };
        let after = SchemaShape { name: "User".into(), fields: BTreeMap::from([("id".into(), "string".into()), ("email".into(), "string".into())]), required: BTreeSet::from(["id".into(), "email".into()]), examples: BTreeMap::new() };
        let drift = diff_schema(&before, &after);
        assert!(drift.breaking());
        assert_eq!(drift.newly_required, vec!["email"]);
    }

    #[test]
    fn service_graph_follows_local_dependencies() {
        let mut graph = ServiceGraph::default();
        for (id, kind) in [("ui", ServiceKind::Frontend), ("api", ServiceKind::Api), ("db", ServiceKind::Database)] {
            graph.add_service(LocalService { id: id.into(), name: id.into(), kind, address: None, process_id: None, project_root: None });
        }
        graph.add_edge(ServiceEdge { from: "ui".into(), to: "api".into(), protocol: "http".into(), evidence: "request".into() });
        graph.add_edge(ServiceEdge { from: "api".into(), to: "db".into(), protocol: "sql".into(), evidence: "trace".into() });
        assert_eq!(graph.reachable_from("ui", 3), vec!["api", "db"]);
    }
}