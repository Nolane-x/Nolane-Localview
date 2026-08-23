#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MutationClass { Layout, Accessibility, Behavior, Failure, Content, Visual }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum MutationOperator {
    Shift { dx: f64, dy: f64 },
    Resize { width_factor: f64, height_factor: f64 },
    Hide,
    RemoveAccessibleName,
    BreakTabOrder,
    DisableHandler,
    DelayFeedback { milliseconds: u64 },
    ForceHttpStatus { status: u16 },
    ForceTimeout { milliseconds: u64 },
    ReplaceContent { value: String },
    TokenOverride { token: String, value: Value },
}

impl MutationOperator {
    pub fn class(&self) -> MutationClass {
        match self {
            Self::Shift { .. } | Self::Resize { .. } | Self::Hide => MutationClass::Layout,
            Self::RemoveAccessibleName | Self::BreakTabOrder => MutationClass::Accessibility,
            Self::DisableHandler | Self::DelayFeedback { .. } => MutationClass::Behavior,
            Self::ForceHttpStatus { .. } | Self::ForceTimeout { .. } => MutationClass::Failure,
            Self::ReplaceContent { .. } => MutationClass::Content,
            Self::TokenOverride { .. } => MutationClass::Visual,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MutationCase {
    pub id: Uuid,
    pub target: String,
    pub operator: MutationOperator,
    pub expected_detectors: BTreeSet<String>,
    pub safe_to_run: bool,
    pub relevance: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationVerdict { Killed, Survived, Invalid, SkippedUnsafe }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationOutcome {
    pub mutation_id: Uuid,
    pub verdict: MutationVerdict,
    pub triggered_detectors: BTreeSet<String>,
    pub evidence_ids: Vec<String>,
}

pub fn evaluate_mutation(case: &MutationCase, triggered_detectors: BTreeSet<String>, evidence_ids: Vec<String>) -> MutationOutcome {
    if !case.safe_to_run {
        return MutationOutcome { mutation_id: case.id, verdict: MutationVerdict::SkippedUnsafe, triggered_detectors, evidence_ids };
    }
    if case.expected_detectors.is_empty() {
        return MutationOutcome { mutation_id: case.id, verdict: MutationVerdict::Invalid, triggered_detectors, evidence_ids };
    }
    let killed = !case.expected_detectors.is_disjoint(&triggered_detectors);
    MutationOutcome { mutation_id: case.id, verdict: if killed { MutationVerdict::Killed } else { MutationVerdict::Survived }, triggered_detectors, evidence_ids }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationQualityMap {
    pub overall_kill_rate: f32,
    pub weighted_kill_rate: f32,
    pub by_class: BTreeMap<MutationClass, f32>,
    pub surviving_mutations: Vec<Uuid>,
    pub unsafe_skipped: usize,
}

pub fn quality_map(cases: &[MutationCase], outcomes: &[MutationOutcome]) -> VerificationQualityMap {
    let outcome_by_id = outcomes.iter().map(|outcome| (outcome.mutation_id, outcome)).collect::<BTreeMap<_, _>>();
    let valid = cases.iter().filter(|case| case.safe_to_run && !case.expected_detectors.is_empty()).collect::<Vec<_>>();
    let killed_count = valid.iter().filter(|case| outcome_by_id.get(&case.id).is_some_and(|outcome| outcome.verdict == MutationVerdict::Killed)).count();
    let total_weight = valid.iter().map(|case| case.relevance.max(0.0)).sum::<f32>();
    let killed_weight = valid.iter().filter(|case| outcome_by_id.get(&case.id).is_some_and(|outcome| outcome.verdict == MutationVerdict::Killed)).map(|case| case.relevance.max(0.0)).sum::<f32>();
    let mut by_class = BTreeMap::new();
    for class in [MutationClass::Layout, MutationClass::Accessibility, MutationClass::Behavior, MutationClass::Failure, MutationClass::Content, MutationClass::Visual] {
        let class_cases = valid.iter().filter(|case| case.operator.class() == class).collect::<Vec<_>>();
        if class_cases.is_empty() { continue; }
        let killed = class_cases.iter().filter(|case| outcome_by_id.get(&case.id).is_some_and(|outcome| outcome.verdict == MutationVerdict::Killed)).count();
        by_class.insert(class, killed as f32 / class_cases.len() as f32);
    }
    let surviving_mutations = valid.iter().filter(|case| !outcome_by_id.get(&case.id).is_some_and(|outcome| outcome.verdict == MutationVerdict::Killed)).map(|case| case.id).collect();
    VerificationQualityMap {
        overall_kill_rate: if valid.is_empty() { 1.0 } else { killed_count as f32 / valid.len() as f32 },
        weighted_kill_rate: if total_weight <= f32::EPSILON { 1.0 } else { killed_weight / total_weight },
        by_class,
        surviving_mutations,
        unsafe_skipped: cases.iter().filter(|case| !case.safe_to_run).count(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationSafetyPolicy {
    pub allow_network_failure: bool,
    pub allow_source_overlay: bool,
    pub allow_external_side_effects: bool,
}

pub fn policy_allows(case: &MutationCase, policy: &MutationSafetyPolicy) -> bool {
    if !case.safe_to_run { return false; }
    match case.operator {
        MutationOperator::ForceHttpStatus { .. } | MutationOperator::ForceTimeout { .. } => policy.allow_network_failure,
        _ => policy.allow_source_overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_is_killed_when_expected_detector_fires() {
        let case = MutationCase { id: Uuid::new_v4(), target: "hero".into(), operator: MutationOperator::Shift { dx: 20.0, dy: 0.0 }, expected_detectors: BTreeSet::from(["layout-diff".into()]), safe_to_run: true, relevance: 1.0 };
        let outcome = evaluate_mutation(&case, BTreeSet::from(["layout-diff".into()]), vec!["ev".into()]);
        assert_eq!(outcome.verdict, MutationVerdict::Killed);
    }

    #[test]
    fn surviving_relevant_mutation_reduces_quality_score() {
        let case = MutationCase { id: Uuid::new_v4(), target: "button".into(), operator: MutationOperator::DisableHandler, expected_detectors: BTreeSet::from(["dead-click".into()]), safe_to_run: true, relevance: 1.0 };
        let outcome = evaluate_mutation(&case, BTreeSet::new(), vec![]);
        let map = quality_map(&[case], &[outcome]);
        assert_eq!(map.overall_kill_rate, 0.0);
    }
}