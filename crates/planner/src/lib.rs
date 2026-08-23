#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_evidence::EvidenceKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PerceptionActionKind {
    SemanticSnapshot,
    ElementInspect,
    RegionCapture,
    ViewportCapture,
    ResponsiveSweep,
    ConsoleRead,
    NetworkRead,
    AccessibilityScan,
    PerformanceSample,
    InteractionReplay,
    ChromiumEscalation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerceptionCandidate {
    pub id: String,
    pub kind: PerceptionActionKind,
    pub target: Option<String>,
    pub expected_evidence: Vec<EvidenceKind>,
    pub uncertainty_reduction: f32,
    pub risk_relevance: f32,
    pub estimated_cpu_ms: u64,
    pub estimated_tokens: usize,
    pub estimated_capture_bytes: usize,
}

impl PerceptionCandidate {
    pub fn information_gain_score(&self) -> f32 {
        let utility = self.uncertainty_reduction.clamp(0.0, 1.0)
            * (0.5 + self.risk_relevance.clamp(0.0, 1.0));
        let cpu_cost = self.estimated_cpu_ms as f32 / 250.0;
        let token_cost = self.estimated_tokens as f32 / 1000.0;
        let storage_cost = self.estimated_capture_bytes as f32 / (1024.0 * 1024.0);
        utility / (1.0 + cpu_cost + token_cost + storage_cost)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerceptionBudget {
    pub max_actions: usize,
    pub max_cpu_ms: u64,
    pub max_tokens: usize,
    pub max_capture_bytes: usize,
    pub allow_chromium: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerceptionPlan {
    pub actions: Vec<PerceptionCandidate>,
    pub cpu_ms: u64,
    pub tokens: usize,
    pub capture_bytes: usize,
    pub rejected: Vec<String>,
}

pub fn plan_perception(candidates: &[PerceptionCandidate], budget: &PerceptionBudget) -> PerceptionPlan {
    let mut ordered = candidates.to_vec();
    ordered.sort_by(|left, right| {
        right
            .information_gain_score()
            .total_cmp(&left.information_gain_score())
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut plan = PerceptionPlan {
        actions: Vec::new(),
        cpu_ms: 0,
        tokens: 0,
        capture_bytes: 0,
        rejected: Vec::new(),
    };
    let mut evidence_covered = BTreeSet::new();

    for candidate in ordered {
        if plan.actions.len() >= budget.max_actions.max(1) {
            plan.rejected.push(candidate.id);
            continue;
        }
        if candidate.kind == PerceptionActionKind::ChromiumEscalation && !budget.allow_chromium {
            plan.rejected.push(candidate.id);
            continue;
        }
        let next_cpu = plan.cpu_ms.saturating_add(candidate.estimated_cpu_ms);
        let next_tokens = plan.tokens.saturating_add(candidate.estimated_tokens);
        let next_capture = plan.capture_bytes.saturating_add(candidate.estimated_capture_bytes);
        if next_cpu > budget.max_cpu_ms
            || next_tokens > budget.max_tokens
            || next_capture > budget.max_capture_bytes
        {
            plan.rejected.push(candidate.id);
            continue;
        }
        let redundant = !candidate.expected_evidence.is_empty()
            && candidate
                .expected_evidence
                .iter()
                .all(|kind| evidence_covered.contains(kind));
        if redundant && candidate.uncertainty_reduction < 0.4 {
            plan.rejected.push(candidate.id);
            continue;
        }
        evidence_covered.extend(candidate.expected_evidence.iter().copied());
        plan.cpu_ms = next_cpu;
        plan.tokens = next_tokens;
        plan.capture_bytes = next_capture;
        plan.actions.push(candidate);
    }
    plan
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RiskSignal {
    pub id: String,
    pub probability: f32,
    pub impact: f32,
    pub confidence: f32,
    pub affected_targets: Vec<String>,
}

impl RiskSignal {
    pub fn score(&self) -> f32 {
        self.probability.clamp(0.0, 1.0)
            * self.impact.clamp(0.0, 1.0)
            * self.confidence.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QaCheck {
    pub id: String,
    pub target: String,
    pub check: String,
    pub priority: u16,
}

pub fn adaptive_qa_plan(signals: &[RiskSignal], checks_by_target: &BTreeMap<String, Vec<String>>, max_checks: usize) -> Vec<QaCheck> {
    let mut signals = signals.to_vec();
    signals.sort_by(|left, right| right.score().total_cmp(&left.score()).then_with(|| left.id.cmp(&right.id)));
    let mut dedupe = BTreeSet::new();
    let mut result = Vec::new();
    for signal in signals {
        for target in signal.affected_targets {
            let Some(checks) = checks_by_target.get(&target) else { continue; };
            for check in checks {
                let key = format!("{target}|{check}");
                if !dedupe.insert(key.clone()) { continue; }
                let priority = (signal.score() * 1000.0).round().clamp(0.0, u16::MAX as f32) as u16;
                result.push(QaCheck { id: key, target: target.clone(), check: check.clone(), priority });
                if result.len() >= max_checks { return result; }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_prefers_high_information_action_within_budget() {
        let candidates = vec![
            PerceptionCandidate { id: "full".into(), kind: PerceptionActionKind::ViewportCapture, target: None, expected_evidence: vec![EvidenceKind::Visual], uncertainty_reduction: 0.8, risk_relevance: 0.8, estimated_cpu_ms: 200, estimated_tokens: 800, estimated_capture_bytes: 2_000_000 },
            PerceptionCandidate { id: "region".into(), kind: PerceptionActionKind::RegionCapture, target: Some("hero".into()), expected_evidence: vec![EvidenceKind::Visual], uncertainty_reduction: 0.7, risk_relevance: 0.9, estimated_cpu_ms: 30, estimated_tokens: 120, estimated_capture_bytes: 100_000 },
        ];
        let plan = plan_perception(&candidates, &PerceptionBudget { max_actions: 1, max_cpu_ms: 300, max_tokens: 1000, max_capture_bytes: 3_000_000, allow_chromium: false });
        assert_eq!(plan.actions[0].id, "region");
    }

    #[test]
    fn chromium_requires_explicit_budget_permission() {
        let candidate = PerceptionCandidate { id: "chromium".into(), kind: PerceptionActionKind::ChromiumEscalation, target: None, expected_evidence: vec![EvidenceKind::Visual], uncertainty_reduction: 1.0, risk_relevance: 1.0, estimated_cpu_ms: 1, estimated_tokens: 1, estimated_capture_bytes: 1 };
        let plan = plan_perception(&[candidate], &PerceptionBudget { max_actions: 1, max_cpu_ms: 10, max_tokens: 10, max_capture_bytes: 10, allow_chromium: false });
        assert!(plan.actions.is_empty());
    }
}