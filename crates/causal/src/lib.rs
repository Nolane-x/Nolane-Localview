#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use localview_evidence::EvidenceId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CausalEntityKind {
    Source,
    Component,
    Region,
    Element,
    StyleToken,
    Asset,
    Route,
    Request,
    Service,
    RuntimeState,
    ConsoleIssue,
    VisualResult,
    Contract,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CausalNode {
    pub id: String,
    pub kind: CausalEntityKind,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CausalRelation {
    Renders,
    Styles,
    Imports,
    Requests,
    Triggers,
    DependsOn,
    Produces,
    Changes,
    MapsTo,
    DerivedFrom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CausalEdge {
    pub from: String,
    pub to: String,
    pub relation: CausalRelation,
    pub confidence: f32,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CausalGraph {
    pub nodes: BTreeMap<String, CausalNode>,
    pub edges: Vec<CausalEdge>,
}

impl CausalGraph {
    pub fn upsert_node(&mut self, node: CausalNode) {
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn add_edge(&mut self, mut edge: CausalEdge) -> bool {
        if !self.nodes.contains_key(&edge.from) || !self.nodes.contains_key(&edge.to) {
            return false;
        }
        edge.confidence = edge.confidence.clamp(0.0, 1.0);
        if let Some(existing) = self.edges.iter_mut().find(|candidate| {
            candidate.from == edge.from
                && candidate.to == edge.to
                && candidate.relation == edge.relation
        }) {
            if edge.confidence > existing.confidence {
                existing.confidence = edge.confidence;
            }
            existing.evidence_ids.extend(edge.evidence_ids);
            existing.evidence_ids.sort();
            existing.evidence_ids.dedup();
            return true;
        }
        self.edges.push(edge);
        true
    }

    pub fn blast_radius(
        &self,
        start: &str,
        max_depth: usize,
        min_confidence: f32,
    ) -> Vec<ImpactNode> {
        if !self.nodes.contains_key(start) {
            return Vec::new();
        }
        let mut queue = VecDeque::from([(start.to_owned(), 0usize, 1.0f32)]);
        let mut best = BTreeMap::<String, ImpactNode>::new();
        let mut visited_depth = BTreeMap::<String, usize>::new();
        visited_depth.insert(start.to_owned(), 0);

        while let Some((current, depth, path_confidence)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for edge in self.edges.iter().filter(|edge| edge.from == current) {
                if edge.confidence < min_confidence {
                    continue;
                }
                let next_depth = depth + 1;
                let next_confidence = path_confidence.min(edge.confidence);
                let candidate = ImpactNode {
                    id: edge.to.clone(),
                    depth: next_depth,
                    confidence: next_confidence,
                    via: edge.relation,
                    evidence_ids: edge.evidence_ids.clone(),
                };
                match best.get_mut(&edge.to) {
                    Some(existing) if candidate.confidence > existing.confidence => {
                        *existing = candidate.clone();
                    }
                    None => {
                        best.insert(edge.to.clone(), candidate.clone());
                    }
                    _ => {}
                }
                let should_visit = visited_depth
                    .get(&edge.to)
                    .is_none_or(|known_depth| next_depth < *known_depth);
                if should_visit {
                    visited_depth.insert(edge.to.clone(), next_depth);
                    queue.push_back((edge.to.clone(), next_depth, next_confidence));
                }
            }
        }

        let mut impacts = best.into_values().collect::<Vec<_>>();
        impacts.sort_by(|left, right| {
            left.depth
                .cmp(&right.depth)
                .then_with(|| right.confidence.total_cmp(&left.confidence))
                .then_with(|| left.id.cmp(&right.id))
        });
        impacts
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImpactNode {
    pub id: String,
    pub depth: usize,
    pub confidence: f32,
    pub via: CausalRelation,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisStatus {
    Unverified,
    Supported,
    Falsified,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeHypothesis {
    pub id: String,
    pub claim: String,
    pub predicted_effects: Vec<String>,
    pub supporting_evidence: Vec<EvidenceId>,
    pub contradicting_evidence: Vec<EvidenceId>,
    pub confidence: f32,
    pub status: HypothesisStatus,
}

impl RuntimeHypothesis {
    pub fn evaluate(&mut self) {
        self.confidence = self.confidence.clamp(0.0, 1.0);
        self.status = match (
            self.supporting_evidence.is_empty(),
            self.contradicting_evidence.is_empty(),
        ) {
            (false, true) => HypothesisStatus::Supported,
            (true, false) => HypothesisStatus::Falsified,
            (false, false) => HypothesisStatus::Inconclusive,
            (true, true) => HypothesisStatus::Unverified,
        };
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationAction {
    Capture,
    SemanticSnapshot,
    ResponsiveSweep,
    Replay,
    NetworkCheck,
    ConsoleCheck,
    AccessibilityCheck,
    PerformanceCheck,
    ContractCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationStep {
    pub id: String,
    pub action: VerificationAction,
    pub target: Option<String>,
    pub expected_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationBudget {
    pub max_steps: usize,
    pub max_capture_regions: usize,
    pub allow_chromium_escalation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationPlan {
    pub hypothesis_id: String,
    pub steps: Vec<VerificationStep>,
    pub budget: VerificationBudget,
}

pub fn targeted_plan(hypothesis: &RuntimeHypothesis, impacts: &[ImpactNode]) -> VerificationPlan {
    let mut regions = impacts
        .iter()
        .filter(|impact| impact.confidence >= 0.65)
        .map(|impact| impact.id.clone())
        .collect::<BTreeSet<_>>();
    for predicted in &hypothesis.predicted_effects {
        regions.insert(predicted.clone());
    }
    let mut steps = Vec::new();
    steps.push(VerificationStep {
        id: "semantic".into(),
        action: VerificationAction::SemanticSnapshot,
        target: None,
        expected_evidence: vec!["state_delta".into()],
    });
    for (index, region) in regions.into_iter().take(8).enumerate() {
        steps.push(VerificationStep {
            id: format!("region-{index}"),
            action: VerificationAction::Capture,
            target: Some(region),
            expected_evidence: vec!["visual_delta".into(), "layout_delta".into()],
        });
    }
    steps.push(VerificationStep {
        id: "runtime".into(),
        action: VerificationAction::ConsoleCheck,
        target: None,
        expected_evidence: vec!["console_delta".into()],
    });
    VerificationPlan {
        hypothesis_id: hypothesis.id.clone(),
        budget: VerificationBudget {
            max_steps: steps.len().min(12),
            max_capture_regions: 8,
            allow_chromium_escalation: false,
        },
        steps,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProofVerdict {
    Pass,
    Fail,
    Inconclusive,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofCapsule {
    pub baseline_revision: String,
    pub candidate_revision: String,
    pub plan_id: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub verdict: ProofVerdict,
    pub reasons: Vec<String>,
}

impl ProofCapsule {
    pub fn is_portable(&self) -> bool {
        !self.baseline_revision.is_empty()
            && !self.candidate_revision.is_empty()
            && !self.plan_id.is_empty()
            && !self.evidence_ids.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> CausalNode {
        CausalNode { id: id.into(), kind: CausalEntityKind::Region, label: id.into() }
    }

    #[test]
    fn blast_radius_respects_confidence_threshold() {
        let mut graph = CausalGraph::default();
        for id in ["source", "component", "hero", "footer"] { graph.upsert_node(node(id)); }
        graph.add_edge(CausalEdge { from: "source".into(), to: "component".into(), relation: CausalRelation::Renders, confidence: 0.95, evidence_ids: vec!["ev1".into()] });
        graph.add_edge(CausalEdge { from: "component".into(), to: "hero".into(), relation: CausalRelation::Renders, confidence: 0.9, evidence_ids: vec!["ev2".into()] });
        graph.add_edge(CausalEdge { from: "source".into(), to: "footer".into(), relation: CausalRelation::DependsOn, confidence: 0.2, evidence_ids: vec!["ev3".into()] });
        let impacts = graph.blast_radius("source", 3, 0.5);
        assert_eq!(impacts.iter().map(|impact| impact.id.as_str()).collect::<Vec<_>>(), vec!["component", "hero"]);
    }

    #[test]
    fn contradictory_evidence_never_becomes_supported() {
        let mut hypothesis = RuntimeHypothesis {
            id: "h1".into(),
            claim: "token change caused overflow".into(),
            predicted_effects: vec!["hero".into()],
            supporting_evidence: vec!["ev1".into()],
            contradicting_evidence: vec!["ev2".into()],
            confidence: 0.8,
            status: HypothesisStatus::Unverified,
        };
        hypothesis.evaluate();
        assert_eq!(hypothesis.status, HypothesisStatus::Inconclusive);
    }
}