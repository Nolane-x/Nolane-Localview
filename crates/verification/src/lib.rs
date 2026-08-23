#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_causal::{ProofCapsule, ProofVerdict};
use localview_contracts::{ContractResult, ContractVerdict};
use localview_evidence::{EvidenceId, EvidenceObject};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageTarget {
    pub id: String,
    pub risk_weight: u16,
    pub required_evidence_classes: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageObservation {
    pub target_id: String,
    pub evidence_classes: BTreeSet<String>,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageReport {
    pub target_coverage: f32,
    pub weighted_coverage: f32,
    pub uncovered_targets: Vec<String>,
    pub weak_targets: Vec<String>,
}

pub fn coverage_report(targets: &[CoverageTarget], observations: &[CoverageObservation]) -> CoverageReport {
    if targets.is_empty() {
        return CoverageReport { target_coverage: 1.0, weighted_coverage: 1.0, uncovered_targets: Vec::new(), weak_targets: Vec::new() };
    }
    let by_target = observations.iter().map(|observation| (observation.target_id.as_str(), observation)).collect::<BTreeMap<_, _>>();
    let total_weight = targets.iter().map(|target| target.risk_weight.max(1) as u64).sum::<u64>();
    let mut complete_count = 0usize;
    let mut covered_weight = 0u64;
    let mut uncovered = Vec::new();
    let mut weak = Vec::new();
    for target in targets {
        match by_target.get(target.id.as_str()) {
            None => uncovered.push(target.id.clone()),
            Some(observation) => {
                let complete = target.required_evidence_classes.is_subset(&observation.evidence_classes);
                if complete {
                    complete_count += 1;
                    covered_weight += target.risk_weight.max(1) as u64;
                } else {
                    weak.push(target.id.clone());
                    let required = target.required_evidence_classes.len().max(1) as u64;
                    let present = target.required_evidence_classes.intersection(&observation.evidence_classes).count() as u64;
                    covered_weight += (target.risk_weight.max(1) as u64 * present) / required;
                }
            }
        }
    }
    CoverageReport {
        target_coverage: complete_count as f32 / targets.len() as f32,
        weighted_coverage: if total_weight == 0 { 1.0 } else { covered_weight as f32 / total_weight as f32 },
        uncovered_targets: uncovered,
        weak_targets: weak,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckSample {
    pub run_id: String,
    pub outcome_hash: String,
    pub environment_hash: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeterminismReport {
    pub determinism_score: Option<f32>,
    pub flaky: bool,
    pub outcome_count: usize,
    pub comparable_samples: usize,
    pub reason: String,
}

pub fn determinism(samples: &[CheckSample]) -> DeterminismReport {
    if samples.len() < 2 {
        return DeterminismReport { determinism_score: None, flaky: false, outcome_count: samples.len(), comparable_samples: samples.len(), reason: "not enough repeated evidence to assign a determinism score".into() };
    }
    let environment = &samples[0].environment_hash;
    let comparable = samples.iter().filter(|sample| &sample.environment_hash == environment).collect::<Vec<_>>();
    if comparable.len() < 2 {
        return DeterminismReport { determinism_score: None, flaky: false, outcome_count: comparable.len(), comparable_samples: comparable.len(), reason: "environment changed between runs".into() };
    }
    let counts = comparable.iter().fold(BTreeMap::<&str, usize>::new(), |mut counts, sample| {
        *counts.entry(sample.outcome_hash.as_str()).or_default() += 1;
        counts
    });
    let dominant = counts.values().copied().max().unwrap_or_default();
    let score = dominant as f32 / comparable.len() as f32;
    DeterminismReport { determinism_score: Some(score), flaky: counts.len() > 1, outcome_count: counts.len(), comparable_samples: comparable.len(), reason: if counts.len() > 1 { "same environment produced multiple outcomes".into() } else { "repeated outcome is stable in the sampled environment".into() } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StalenessContext {
    pub current_revision: String,
    pub current_environment_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceFreshness {
    pub evidence_id: EvidenceId,
    pub stale: bool,
    pub reasons: Vec<String>,
}

pub fn evidence_freshness(evidence: &EvidenceObject, context: &StalenessContext) -> EvidenceFreshness {
    let mut reasons = Vec::new();
    if evidence.provenance.revision.as_deref().is_some_and(|revision| revision != context.current_revision) {
        reasons.push("revision changed".into());
    }
    if evidence.provenance.engine.as_deref().is_some_and(|engine| engine.starts_with("env:") && engine.trim_start_matches("env:") != context.current_environment_hash) {
        reasons.push("environment changed".into());
    }
    EvidenceFreshness { evidence_id: evidence.id.clone(), stale: !reasons.is_empty(), reasons }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProofQuality {
    pub score: f32,
    pub accepted: bool,
    pub reasons: Vec<String>,
}

pub fn assess_proof(
    proof: &ProofCapsule,
    evidence: &[EvidenceObject],
    contracts: &[ContractResult],
    minimum_confidence: f32,
) -> ProofQuality {
    let by_id = evidence.iter().map(|item| (item.id.as_str(), item)).collect::<BTreeMap<_, _>>();
    let mut reasons = Vec::new();
    let missing = proof.evidence_ids.iter().filter(|id| !by_id.contains_key(id.as_str())).count();
    if missing > 0 { reasons.push(format!("{missing} proof evidence object(s) are missing")); }
    let tainted = proof.evidence_ids.iter().filter_map(|id| by_id.get(id.as_str())).filter(|item| item.secret_taint).count();
    if tainted > 0 { reasons.push(format!("{tainted} proof evidence object(s) contain secret taint")); }
    let confidences = proof.evidence_ids.iter().filter_map(|id| by_id.get(id.as_str())).map(|item| item.confidence).collect::<Vec<_>>();
    let mean_confidence = if confidences.is_empty() { 0.0 } else { confidences.iter().copied().sum::<f32>() / confidences.len() as f32 };
    if mean_confidence < minimum_confidence { reasons.push(format!("mean evidence confidence {mean_confidence:.2} is below required {minimum_confidence:.2}")); }
    let hard_contract_failures = contracts.iter().filter(|result| result.verdict == ContractVerdict::Fail).count();
    if hard_contract_failures > 0 { reasons.push(format!("{hard_contract_failures} executable contract(s) failed")); }
    let verdict_factor = match proof.verdict { ProofVerdict::Pass => 1.0, ProofVerdict::Inconclusive => 0.5, ProofVerdict::Stale => 0.2, ProofVerdict::Fail => 0.0 };
    let completeness = if proof.evidence_ids.is_empty() { 0.0 } else { (proof.evidence_ids.len().saturating_sub(missing)) as f32 / proof.evidence_ids.len() as f32 };
    let score = (verdict_factor * 0.35 + completeness * 0.25 + mean_confidence * 0.4).clamp(0.0, 1.0);
    let accepted = proof.verdict == ProofVerdict::Pass && missing == 0 && tainted == 0 && hard_contract_failures == 0 && mean_confidence >= minimum_confidence;
    ProofQuality { score, accepted, reasons }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegressionDecision {
    pub reject: bool,
    pub reasons: Vec<String>,
}

pub fn regression_decision(
    baseline_failures: &BTreeSet<String>,
    candidate_failures: &BTreeSet<String>,
    proof_quality: &ProofQuality,
) -> RegressionDecision {
    let new_failures = candidate_failures.difference(baseline_failures).cloned().collect::<Vec<_>>();
    let mut reasons = new_failures.iter().map(|failure| format!("new failure: {failure}")).collect::<Vec<_>>();
    if !proof_quality.accepted { reasons.push("candidate does not carry an acceptable proof".into()); }
    RegressionDecision { reject: !new_failures.is_empty() || !proof_quality.accepted, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_different_hashes_in_same_environment_are_flaky() {
        let samples = vec![
            CheckSample { run_id: "1".into(), outcome_hash: "a".into(), environment_hash: "env".into(), passed: true },
            CheckSample { run_id: "2".into(), outcome_hash: "b".into(), environment_hash: "env".into(), passed: false },
        ];
        let report = determinism(&samples);
        assert!(report.flaky);
        assert_eq!(report.determinism_score, Some(0.5));
    }

    #[test]
    fn weighted_coverage_penalizes_missing_high_risk_target() {
        let targets = vec![
            CoverageTarget { id: "checkout".into(), risk_weight: 10, required_evidence_classes: BTreeSet::from(["behavior".into()]) },
            CoverageTarget { id: "footer".into(), risk_weight: 1, required_evidence_classes: BTreeSet::from(["visual".into()]) },
        ];
        let observations = vec![CoverageObservation { target_id: "footer".into(), evidence_classes: BTreeSet::from(["visual".into()]), evidence_ids: vec!["ev".into()] }];
        let report = coverage_report(&targets, &observations);
        assert!(report.weighted_coverage < 0.2);
        assert_eq!(report.uncovered_targets, vec!["checkout"]);
    }
}