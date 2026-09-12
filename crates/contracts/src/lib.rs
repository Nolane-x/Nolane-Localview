#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_evidence::EvidenceId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractStrength { Hard, Soft }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ContractCategory {
    Layout,
    Accessibility,
    Interaction,
    Content,
    Responsive,
    Performance,
    Network,
    Visual,
    Safety,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractScope {
    pub routes: Vec<String>,
    pub regions: Vec<String>,
    pub personas: Vec<String>,
    pub viewports: Vec<String>,
}

impl ContractScope {
    pub fn global() -> Self { Self { routes: Vec::new(), regions: Vec::new(), personas: Vec::new(), viewports: Vec::new() } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "predicate", rename_all = "snake_case")]
pub enum ContractPredicate {
    Exists { selector: String },
    NotExists { selector: String },
    MetricAtMost { metric: String, max: f64 },
    MetricAtLeast { metric: String, min: f64 },
    Equals { key: String, value: Value },
    NoIssueCode { code: String },
    EveryInteractiveNamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UxContract {
    pub id: String,
    pub title: String,
    pub category: ContractCategory,
    pub strength: ContractStrength,
    pub scope: ContractScope,
    pub predicate: ContractPredicate,
    pub provenance: String,
    pub inherited_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractException {
    pub contract_id: String,
    pub scope_key: String,
    pub reason: String,
    pub approved_by: String,
    pub expires_revision: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RuntimeFacts {
    pub selectors: BTreeSet<String>,
    pub metrics: BTreeMap<String, f64>,
    pub values: BTreeMap<String, Value>,
    pub issue_codes: BTreeSet<String>,
    pub unnamed_interactive_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractVerdict { Pass, Fail, Excepted, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractResult {
    pub contract_id: String,
    pub verdict: ContractVerdict,
    pub explanation: String,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Default)]
pub struct ContractRegistry {
    contracts: BTreeMap<String, UxContract>,
}

impl ContractRegistry {
    pub fn insert(&mut self, contract: UxContract) { self.contracts.insert(contract.id.clone(), contract); }

    pub fn get(&self, id: &str) -> Option<&UxContract> { self.contracts.get(id) }

    pub fn effective_contract(&self, id: &str) -> Result<UxContract, ContractCompileError> {
        let mut current = self.contracts.get(id).cloned().ok_or_else(|| ContractCompileError::MissingContract(id.into()))?;
        let mut visited = BTreeSet::new();
        visited.insert(current.id.clone());
        while let Some(parent_id) = current.inherited_from.clone() {
            if !visited.insert(parent_id.clone()) { return Err(ContractCompileError::InheritanceCycle(parent_id)); }
            let parent = self.contracts.get(&parent_id).ok_or_else(|| ContractCompileError::MissingParent(parent_id.clone()))?;
            current.scope = merge_scope(&parent.scope, &current.scope);
            if current.provenance.is_empty() { current.provenance = parent.provenance.clone(); }
            current.inherited_from = parent.inherited_from.clone();
        }
        Ok(current)
    }

    pub fn conflicts(&self) -> Vec<ContractConflict> {
        let contracts = self.contracts.values().collect::<Vec<_>>();
        let mut conflicts = Vec::new();
        for (index, left) in contracts.iter().enumerate() {
            for right in contracts.iter().skip(index + 1) {
                if left.scope == right.scope && predicates_conflict(&left.predicate, &right.predicate) {
                    conflicts.push(ContractConflict { left: left.id.clone(), right: right.id.clone(), reason: "same scope contains incompatible predicates".into() });
                }
            }
        }
        conflicts
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractCompileError { MissingContract(String), MissingParent(String), InheritanceCycle(String) }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractConflict { pub left: String, pub right: String, pub reason: String }

pub fn evaluate(
    contract: &UxContract,
    facts: &RuntimeFacts,
    exception: Option<&ContractException>,
    evidence_ids: Vec<EvidenceId>,
) -> ContractResult {
    if exception.is_some_and(|item| item.contract_id == contract.id) {
        return ContractResult { contract_id: contract.id.clone(), verdict: ContractVerdict::Excepted, explanation: "approved exception applies".into(), evidence_ids };
    }
    let (verdict, explanation) = match &contract.predicate {
        ContractPredicate::Exists { selector } => bool_result(facts.selectors.contains(selector), format!("selector {selector} exists"), format!("selector {selector} missing")),
        ContractPredicate::NotExists { selector } => bool_result(!facts.selectors.contains(selector), format!("selector {selector} absent"), format!("selector {selector} unexpectedly exists")),
        ContractPredicate::MetricAtMost { metric, max } => match facts.metrics.get(metric) {
            Some(value) => bool_result(*value <= *max, format!("{metric}={value} <= {max}"), format!("{metric}={value} > {max}")),
            None => (ContractVerdict::Unknown, format!("metric {metric} unavailable")),
        },
        ContractPredicate::MetricAtLeast { metric, min } => match facts.metrics.get(metric) {
            Some(value) => bool_result(*value >= *min, format!("{metric}={value} >= {min}"), format!("{metric}={value} < {min}")),
            None => (ContractVerdict::Unknown, format!("metric {metric} unavailable")),
        },
        ContractPredicate::Equals { key, value } => match facts.values.get(key) {
            Some(actual) => bool_result(actual == value, format!("{key} matches contract value"), format!("{key} differs from contract value")),
            None => (ContractVerdict::Unknown, format!("value {key} unavailable")),
        },
        ContractPredicate::NoIssueCode { code } => bool_result(!facts.issue_codes.contains(code), format!("issue {code} absent"), format!("issue {code} present")),
        ContractPredicate::EveryInteractiveNamed => bool_result(facts.unnamed_interactive_count == 0, "all interactive elements have accessible names".into(), format!("{} interactive element(s) are unnamed", facts.unnamed_interactive_count)),
    };
    ContractResult { contract_id: contract.id.clone(), verdict, explanation, evidence_ids }
}

fn bool_result(pass: bool, pass_message: String, fail_message: String) -> (ContractVerdict, String) {
    if pass { (ContractVerdict::Pass, pass_message) } else { (ContractVerdict::Fail, fail_message) }
}

fn merge_scope(parent: &ContractScope, child: &ContractScope) -> ContractScope {
    ContractScope {
        routes: choose(&parent.routes, &child.routes),
        regions: choose(&parent.regions, &child.regions),
        personas: choose(&parent.personas, &child.personas),
        viewports: choose(&parent.viewports, &child.viewports),
    }
}

fn choose(parent: &[String], child: &[String]) -> Vec<String> { if child.is_empty() { parent.to_vec() } else { child.to_vec() } }

fn predicates_conflict(left: &ContractPredicate, right: &ContractPredicate) -> bool {
    match (left, right) {
        (ContractPredicate::Exists { selector: a }, ContractPredicate::NotExists { selector: b })
        | (ContractPredicate::NotExists { selector: a }, ContractPredicate::Exists { selector: b }) => a == b,
        (ContractPredicate::MetricAtMost { metric: a, max }, ContractPredicate::MetricAtLeast { metric: b, min })
        | (ContractPredicate::MetricAtLeast { metric: a, min }, ContractPredicate::MetricAtMost { metric: b, max }) => a == b && min > max,
        (ContractPredicate::Equals { key: a, value: av }, ContractPredicate::Equals { key: b, value: bv }) => a == b && av != bv,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_metric_contract_fails_deterministically() {
        let contract = UxContract { id: "perf.lcp".into(), title: "LCP budget".into(), category: ContractCategory::Performance, strength: ContractStrength::Hard, scope: ContractScope::global(), predicate: ContractPredicate::MetricAtMost { metric: "lcp_ms".into(), max: 2500.0 }, provenance: "project policy".into(), inherited_from: None };
        let facts = RuntimeFacts { metrics: BTreeMap::from([("lcp_ms".into(), 3100.0)]), ..Default::default() };
        assert_eq!(evaluate(&contract, &facts, None, vec![]).verdict, ContractVerdict::Fail);
    }

    #[test]
    fn registry_detects_simple_exists_conflict() {
        let scope = ContractScope::global();
        let mut registry = ContractRegistry::default();
        registry.insert(UxContract { id: "a".into(), title: "a".into(), category: ContractCategory::Layout, strength: ContractStrength::Hard, scope: scope.clone(), predicate: ContractPredicate::Exists { selector: "#hero".into() }, provenance: "test".into(), inherited_from: None });
        registry.insert(UxContract { id: "b".into(), title: "b".into(), category: ContractCategory::Layout, strength: ContractStrength::Hard, scope, predicate: ContractPredicate::NotExists { selector: "#hero".into() }, provenance: "test".into(), inherited_from: None });
        assert_eq!(registry.conflicts().len(), 1);
    }
}