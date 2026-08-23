#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_evidence::EvidenceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IsolationLevel { SemanticOnly, NativeWebView, ChromiumSandbox }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceOverlay {
    pub file: String,
    pub base_hash: String,
    pub patch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CounterfactualCandidate {
    pub id: Uuid,
    pub name: String,
    pub base_revision: String,
    pub overlays: Vec<SourceOverlay>,
    pub isolation: IsolationLevel,
    pub disposable: bool,
    pub evidence_ids: Vec<EvidenceId>,
    pub metrics: BTreeMap<String, f64>,
    pub hard_failures: BTreeSet<String>,
}

impl CounterfactualCandidate {
    pub fn validate(&self) -> Result<(), CandidateError> {
        if self.base_revision.trim().is_empty() { return Err(CandidateError::MissingBaseRevision); }
        if !self.disposable { return Err(CandidateError::NotDisposable); }
        if self.overlays.iter().any(|overlay| overlay.base_hash.trim().is_empty()) { return Err(CandidateError::UnboundOverlay); }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CandidateError { MissingBaseRevision, NotDisposable, UnboundOverlay }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Objective {
    pub metric: String,
    pub direction: ObjectiveDirection,
    pub weight: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveDirection { Minimize, Maximize }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateScore {
    pub candidate_id: Uuid,
    pub score: f64,
    pub hard_failures: usize,
    pub missing_metrics: Vec<String>,
}

pub fn score(candidate: &CounterfactualCandidate, objectives: &[Objective]) -> CandidateScore {
    let mut total = 0.0;
    let mut missing = Vec::new();
    for objective in objectives {
        match candidate.metrics.get(&objective.metric) {
            Some(value) => {
                let signed = match objective.direction { ObjectiveDirection::Minimize => -*value, ObjectiveDirection::Maximize => *value };
                total += signed * objective.weight.max(0.0);
            }
            None => missing.push(objective.metric.clone()),
        }
    }
    CandidateScore { candidate_id: candidate.id, score: total, hard_failures: candidate.hard_failures.len(), missing_metrics: missing }
}

pub fn dominates(left: &CounterfactualCandidate, right: &CounterfactualCandidate, objectives: &[Objective]) -> bool {
    if !left.hard_failures.is_empty() && right.hard_failures.is_empty() { return false; }
    if left.hard_failures.is_empty() && !right.hard_failures.is_empty() { return true; }
    let mut strictly_better = false;
    for objective in objectives {
        let (Some(left_value), Some(right_value)) = (left.metrics.get(&objective.metric), right.metrics.get(&objective.metric)) else { return false; };
        match objective.direction {
            ObjectiveDirection::Minimize => {
                if left_value > right_value { return false; }
                strictly_better |= left_value < right_value;
            }
            ObjectiveDirection::Maximize => {
                if left_value < right_value { return false; }
                strictly_better |= left_value > right_value;
            }
        }
    }
    strictly_better
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TournamentResult {
    pub ranked: Vec<CandidateScore>,
    pub pareto_front: Vec<Uuid>,
    pub recommended: Option<Uuid>,
    pub explanation: Vec<String>,
}

pub fn tournament(candidates: &[CounterfactualCandidate], objectives: &[Objective]) -> TournamentResult {
    let mut ranked = candidates.iter().map(|candidate| score(candidate, objectives)).collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.hard_failures.cmp(&right.hard_failures).then_with(|| right.score.total_cmp(&left.score)));
    let pareto_front = candidates.iter().filter(|candidate| !candidates.iter().any(|other| other.id != candidate.id && dominates(other, candidate, objectives))).map(|candidate| candidate.id).collect::<Vec<_>>();
    let recommended = ranked.iter().find(|candidate| candidate.hard_failures == 0 && candidate.missing_metrics.is_empty()).map(|candidate| candidate.candidate_id);
    let mut explanation = Vec::new();
    if let Some(id) = recommended { explanation.push(format!("candidate {id} has no hard failures and the strongest weighted objective score among complete candidates")); }
    if recommended.is_none() { explanation.push("no candidate has complete evidence without hard failures".into()); }
    TournamentResult { ranked, pareto_front, recommended, explanation }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PreflightRisk { Low, Medium, High, Critical }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreflightInput {
    pub changed_files: usize,
    pub impacted_regions: usize,
    pub impacted_routes: usize,
    pub external_side_effects: bool,
    pub schema_change: bool,
    pub evidence_confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreflightReport {
    pub risk: PreflightRisk,
    pub score: u32,
    pub reasons: Vec<String>,
    pub verification_budget_multiplier: f32,
}

pub fn preflight(input: &PreflightInput) -> PreflightReport {
    let mut score = 0u32;
    let mut reasons = Vec::new();
    score += (input.changed_files.min(20) as u32) * 2;
    score += input.impacted_regions.min(30) as u32;
    score += (input.impacted_routes.min(10) as u32) * 3;
    if input.external_side_effects { score += 30; reasons.push("external side effects detected".into()); }
    if input.schema_change { score += 25; reasons.push("schema boundary changes".into()); }
    if input.evidence_confidence < 0.6 { score += 20; reasons.push("low-confidence impact evidence".into()); }
    if input.impacted_routes > 3 { reasons.push("change spans multiple routes".into()); }
    let risk = match score { 0..=19 => PreflightRisk::Low, 20..=44 => PreflightRisk::Medium, 45..=74 => PreflightRisk::High, _ => PreflightRisk::Critical };
    let verification_budget_multiplier = match risk { PreflightRisk::Low => 1.0, PreflightRisk::Medium => 1.5, PreflightRisk::High => 2.5, PreflightRisk::Critical => 4.0 };
    PreflightReport { risk, score, reasons, verification_budget_multiplier }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, lcp: f64, a11y: f64) -> CounterfactualCandidate {
        CounterfactualCandidate { id: Uuid::new_v4(), name: name.into(), base_revision: "abc".into(), overlays: vec![SourceOverlay { file: "App.tsx".into(), base_hash: "hash".into(), patch: "patch".into() }], isolation: IsolationLevel::SemanticOnly, disposable: true, evidence_ids: vec!["ev".into()], metrics: BTreeMap::from([("lcp".into(), lcp), ("a11y".into(), a11y)]), hard_failures: BTreeSet::new() }
    }

    #[test]
    fn pareto_dominance_requires_no_regression_on_any_objective() {
        let better = candidate("better", 1200.0, 100.0);
        let worse = candidate("worse", 1800.0, 90.0);
        let objectives = vec![Objective { metric: "lcp".into(), direction: ObjectiveDirection::Minimize, weight: 1.0 }, Objective { metric: "a11y".into(), direction: ObjectiveDirection::Maximize, weight: 1.0 }];
        assert!(dominates(&better, &worse, &objectives));
    }

    #[test]
    fn destructive_preflight_escalates_risk() {
        let report = preflight(&PreflightInput { changed_files: 8, impacted_regions: 12, impacted_routes: 5, external_side_effects: true, schema_change: true, evidence_confidence: 0.4 });
        assert_eq!(report.risk, PreflightRisk::Critical);
        assert!(report.verification_budget_multiplier >= 4.0);
    }
}