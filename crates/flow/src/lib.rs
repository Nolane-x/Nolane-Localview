#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use localview_protocol::ElementRef;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_FOCUS_TRANSITIONS: usize = 64;
pub const DEFAULT_MAX_GRAPH_NODES: usize = 64;
pub const DEFAULT_MAX_GRAPH_EDGES: usize = 128;
pub const DEFAULT_MAX_ROUTE_STATES: usize = 8;
pub const DEFAULT_DEADLINE_MS: u64 = 8_000;
pub const MAX_EVIDENCE_REFS_PER_EDGE: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Step {
    Navigate { url: String },
    Click { reference: String },
    Type { reference: String, text: String },
    Key { key: String },
    WaitRoute { route: String },
    AssertVisible { reference: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Flow {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateIdentity {
    pub route: String,
    pub document_generation: u64,
    pub semantic_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<(u32, u32)>,
}

impl StateIdentity {
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.route,
            self.document_generation,
            self.semantic_fingerprint,
            self.viewport
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_else(|| "-".into())
        )
    }

    pub fn compatible_with(&self, actual: &Self) -> bool {
        self.route == actual.route
            && self.document_generation == actual.document_generation
            && self.semantic_fingerprint == actual.semantic_fingerprint
            && self.viewport == actual.viewport
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionActionKind {
    Click,
    Focus,
    Tab,
    ShiftTab,
    Key,
    Scroll,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SafetyClass {
    ExplicitlySafe,
    ReadOnly,
    Unknown,
    Destructive,
}

impl SafetyClass {
    pub fn may_probe(self) -> bool {
        matches!(self, Self::ExplicitlySafe | Self::ReadOnly)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub action: String,
    pub target: String,
    pub resulting_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveTransition {
    pub action: InteractionActionKind,
    pub target: ElementRef,
    pub safety: SafetyClass,
    pub pre_state: StateIdentity,
    pub resulting_state: StateIdentity,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InteractionGraph {
    pub edges: BTreeMap<String, Vec<Transition>>,
    #[serde(default)]
    pub live_edges: BTreeMap<String, Vec<LiveTransition>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryBounds {
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_route_states: usize,
    pub deadline_ms: u64,
}

impl Default for DiscoveryBounds {
    fn default() -> Self {
        Self {
            max_nodes: DEFAULT_MAX_GRAPH_NODES,
            max_edges: DEFAULT_MAX_GRAPH_EDGES,
            max_route_states: DEFAULT_MAX_ROUTE_STATES,
            deadline_ms: DEFAULT_DEADLINE_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphAdmissionError {
    NodeBudgetExceeded,
    EdgeBudgetExceeded,
    RouteStateBudgetExceeded,
    UnsafeAction,
    PreStateMismatch,
    InvalidStableReference,
}

impl InteractionGraph {
    pub fn record(&mut self, state: &str, transition: Transition) {
        let edges = self.edges.entry(state.into()).or_default();
        if !edges.iter().any(|edge| {
            edge.action == transition.action && edge.resulting_state == transition.resulting_state
        }) {
            edges.push(transition);
        }
    }

    pub fn record_live(
        &mut self,
        transition: LiveTransition,
        bounds: DiscoveryBounds,
    ) -> Result<(), GraphAdmissionError> {
        if !transition.safety.may_probe() {
            return Err(GraphAdmissionError::UnsafeAction);
        }
        if !valid_stable_reference(&transition.target) {
            return Err(GraphAdmissionError::InvalidStableReference);
        }
        let all_states = self.live_states();
        if !all_states.contains(&transition.pre_state.key())
            && all_states.len() >= bounds.max_nodes
        {
            return Err(GraphAdmissionError::NodeBudgetExceeded);
        }
        if !all_states.contains(&transition.resulting_state.key())
            && all_states.len().saturating_add(1) >= bounds.max_nodes
        {
            return Err(GraphAdmissionError::NodeBudgetExceeded);
        }
        if self.live_edge_count() >= bounds.max_edges {
            return Err(GraphAdmissionError::EdgeBudgetExceeded);
        }
        let routes = self
            .live_edges
            .values()
            .flatten()
            .flat_map(|edge| [&edge.pre_state.route, &edge.resulting_state.route])
            .collect::<BTreeSet<_>>();
        let mut next_routes = routes;
        next_routes.insert(&transition.pre_state.route);
        next_routes.insert(&transition.resulting_state.route);
        if next_routes.len() > bounds.max_route_states {
            return Err(GraphAdmissionError::RouteStateBudgetExceeded);
        }

        let key = transition.pre_state.key();
        let edges = self.live_edges.entry(key).or_default();
        if !edges.iter().any(|edge| {
            edge.action == transition.action
                && edge.target == transition.target
                && edge.resulting_state == transition.resulting_state
        }) {
            let mut transition = transition;
            transition.evidence_refs.truncate(MAX_EVIDENCE_REFS_PER_EDGE);
            edges.push(transition);
        }
        Ok(())
    }

    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<Transition>> {
        let mut queue = VecDeque::from([(from.to_string(), Vec::new())]);
        let mut seen = HashSet::new();
        while let Some((state, path)) = queue.pop_front() {
            if state == to {
                return Some(path);
            }
            if !seen.insert(state.clone()) {
                continue;
            }
            for edge in self.edges.get(&state).into_iter().flatten() {
                let mut next = path.clone();
                next.push(edge.clone());
                queue.push_back((edge.resulting_state.clone(), next));
            }
        }
        None
    }

    pub fn live_edge_count(&self) -> usize {
        self.live_edges.values().map(Vec::len).sum()
    }

    fn live_states(&self) -> BTreeSet<String> {
        self.live_edges
            .values()
            .flatten()
            .flat_map(|edge| [edge.pre_state.key(), edge.resulting_state.key()])
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusObservation {
    pub transition_index: usize,
    pub reference: Option<ElementRef>,
    pub route: String,
    pub document_generation: u64,
    pub tabindex: Option<i32>,
    pub hidden_or_offscreen: bool,
    pub is_body_or_document: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusIssueKind {
    Loop,
    LostToDocument,
    RepeatedFocus,
    PositiveTabindexOrdering,
    HiddenOrOffscreenFocused,
    RouteDrift,
    DocumentGenerationDrift,
    FocusTrap,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusIssue {
    pub kind: FocusIssueKind,
    pub at_transition: usize,
    pub reference: Option<ElementRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyboardJourneyReceipt {
    pub initial: FocusObservation,
    pub transitions: Vec<FocusObservation>,
    pub issues: Vec<FocusIssue>,
    pub complete: bool,
    pub stopped_reason: Option<String>,
}

pub fn analyze_keyboard_journey(
    initial: FocusObservation,
    transitions: Vec<FocusObservation>,
    max_transitions: usize,
) -> KeyboardJourneyReceipt {
    let max_transitions = max_transitions.min(DEFAULT_MAX_FOCUS_TRANSITIONS);
    let mut issues = Vec::new();
    let mut seen: BTreeMap<ElementRef, usize> = BTreeMap::new();
    let mut bounded = transitions;
    bounded.truncate(max_transitions);

    for observation in &bounded {
        if observation.route != initial.route {
            issues.push(FocusIssue {
                kind: FocusIssueKind::RouteDrift,
                at_transition: observation.transition_index,
                reference: observation.reference.clone(),
            });
            return KeyboardJourneyReceipt {
                initial,
                transitions: bounded,
                issues,
                complete: false,
                stopped_reason: Some("route_drift".into()),
            };
        }
        if observation.document_generation != initial.document_generation {
            issues.push(FocusIssue {
                kind: FocusIssueKind::DocumentGenerationDrift,
                at_transition: observation.transition_index,
                reference: observation.reference.clone(),
            });
            return KeyboardJourneyReceipt {
                initial,
                transitions: bounded,
                issues,
                complete: false,
                stopped_reason: Some("document_generation_drift".into()),
            };
        }
        if observation.is_body_or_document {
            issues.push(FocusIssue {
                kind: FocusIssueKind::LostToDocument,
                at_transition: observation.transition_index,
                reference: None,
            });
        }
        if observation.hidden_or_offscreen {
            issues.push(FocusIssue {
                kind: FocusIssueKind::HiddenOrOffscreenFocused,
                at_transition: observation.transition_index,
                reference: observation.reference.clone(),
            });
        }
        if observation.tabindex.is_some_and(|value| value > 0) {
            issues.push(FocusIssue {
                kind: FocusIssueKind::PositiveTabindexOrdering,
                at_transition: observation.transition_index,
                reference: observation.reference.clone(),
            });
        }
        if let Some(reference) = &observation.reference {
            if let Some(first) = seen.insert(reference.clone(), observation.transition_index) {
                issues.push(FocusIssue {
                    kind: if observation.transition_index.saturating_sub(first) <= 2 {
                        FocusIssueKind::RepeatedFocus
                    } else {
                        FocusIssueKind::Loop
                    },
                    at_transition: observation.transition_index,
                    reference: Some(reference.clone()),
                });
            }
        }
    }

    let unique_refs = bounded
        .iter()
        .filter_map(|observation| observation.reference.as_ref())
        .collect::<BTreeSet<_>>();
    if bounded.len() >= 4 && unique_refs.len() == 1 {
        issues.push(FocusIssue {
            kind: FocusIssueKind::FocusTrap,
            at_transition: bounded.last().map_or(0, |item| item.transition_index),
            reference: bounded.last().and_then(|item| item.reference.clone()),
        });
    }

    KeyboardJourneyReceipt {
        initial,
        complete: transitions_within_limit(&bounded, max_transitions),
        transitions: bounded,
        issues,
        stopped_reason: None,
    }
}

fn transitions_within_limit(transitions: &[FocusObservation], limit: usize) -> bool {
    transitions.len() <= limit
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackVerdict {
    ObservedFeedback,
    DelayedFeedback,
    NoObservedFeedback,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FeedbackSignals {
    pub focus_changed: bool,
    pub semantic_changed: bool,
    pub route_changed: bool,
    pub network_activity: bool,
    pub layout_changed: bool,
    pub accessible_state_changed: bool,
}

impl FeedbackSignals {
    pub fn any(&self) -> bool {
        self.focus_changed
            || self.semantic_changed
            || self.route_changed
            || self.network_activity
            || self.layout_changed
            || self.accessible_state_changed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeedbackReceipt {
    pub verdict: FeedbackVerdict,
    pub elapsed_ms: u64,
    pub signals: FeedbackSignals,
    pub safe_interaction: bool,
}

pub fn classify_feedback(
    safe_interaction: bool,
    elapsed_ms: u64,
    deadline_ms: u64,
    delayed_threshold_ms: u64,
    signals: FeedbackSignals,
) -> FeedbackReceipt {
    let verdict = if !safe_interaction || elapsed_ms > deadline_ms {
        FeedbackVerdict::Inconclusive
    } else if signals.any() && elapsed_ms > delayed_threshold_ms {
        FeedbackVerdict::DelayedFeedback
    } else if signals.any() {
        FeedbackVerdict::ObservedFeedback
    } else {
        FeedbackVerdict::NoObservedFeedback
    };
    FeedbackReceipt {
        verdict,
        elapsed_ms,
        signals,
        safe_interaction,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayStep {
    pub index: usize,
    pub action: InteractionActionKind,
    pub target: ElementRef,
    pub expected_before: StateIdentity,
    pub expected_after: StateIdentity,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStatus {
    Complete,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayReceipt {
    pub steps_attempted: usize,
    pub steps_passed: usize,
    pub first_failed_step: Option<usize>,
    pub before_state: StateIdentity,
    pub after_state: StateIdentity,
    pub evidence_refs: Vec<String>,
    pub status: ReplayStatus,
    pub reason: Option<String>,
}

pub fn replay_receipt(
    plan: &[ReplayStep],
    observed: &[StateIdentity],
    refs_valid: &[bool],
) -> ReplayReceipt {
    let before_state = plan
        .first()
        .map(|step| step.expected_before.clone())
        .or_else(|| observed.first().cloned())
        .unwrap_or_else(empty_state);
    let mut evidence_refs = Vec::new();

    if plan.is_empty() {
        return ReplayReceipt {
            steps_attempted: 0,
            steps_passed: 0,
            first_failed_step: None,
            before_state: before_state.clone(),
            after_state: before_state,
            evidence_refs,
            status: ReplayStatus::Inconclusive,
            reason: Some("empty_plan".into()),
        };
    }

    let mut passed = 0usize;
    for (index, step) in plan.iter().enumerate() {
        if refs_valid.get(index).copied() != Some(true) {
            return failed_replay(plan, observed, &evidence_refs, index, passed, "stable_ref_invalid");
        }
        let Some(before) = observed.get(index) else {
            return failed_replay(plan, observed, &evidence_refs, index, passed, "missing_before_state");
        };
        if !step.expected_before.compatible_with(before) {
            return failed_replay(plan, observed, &evidence_refs, index, passed, "pre_state_mismatch");
        }
        let Some(after) = observed.get(index + 1) else {
            return failed_replay(plan, observed, &evidence_refs, index, passed, "missing_after_state");
        };
        if !step.expected_after.compatible_with(after) {
            return failed_replay(plan, observed, &evidence_refs, index, passed, "post_state_mismatch");
        }
        evidence_refs.extend(step.evidence_refs.iter().take(MAX_EVIDENCE_REFS_PER_EDGE).cloned());
        passed += 1;
    }

    ReplayReceipt {
        steps_attempted: plan.len(),
        steps_passed: passed,
        first_failed_step: None,
        before_state,
        after_state: observed
            .last()
            .cloned()
            .unwrap_or_else(|| plan.last().unwrap().expected_after.clone()),
        evidence_refs,
        status: ReplayStatus::Complete,
        reason: None,
    }
}

fn failed_replay(
    plan: &[ReplayStep],
    observed: &[StateIdentity],
    evidence_refs: &[String],
    failed: usize,
    passed: usize,
    reason: &str,
) -> ReplayReceipt {
    let before_state = plan[0].expected_before.clone();
    let after_state = observed
        .get(failed)
        .cloned()
        .unwrap_or_else(|| before_state.clone());
    ReplayReceipt {
        steps_attempted: failed + 1,
        steps_passed: passed,
        first_failed_step: Some(failed),
        before_state,
        after_state,
        evidence_refs: evidence_refs.to_vec(),
        status: ReplayStatus::Failed,
        reason: Some(reason.into()),
    }
}

fn empty_state() -> StateIdentity {
    StateIdentity {
        route: String::new(),
        document_generation: 0,
        semantic_fingerprint: String::new(),
        viewport: None,
    }
}

fn valid_stable_reference(reference: &str) -> bool {
    let Some(rest) = reference.strip_prefix("@e") else {
        return false;
    };
    !rest.is_empty() && rest.len() <= 64 && rest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(route: &str, generation: u64, fingerprint: &str) -> StateIdentity {
        StateIdentity {
            route: route.into(),
            document_generation: generation,
            semantic_fingerprint: fingerprint.into(),
            viewport: Some((800, 600)),
        }
    }

    #[test]
    fn legacy_shortest_path_still_works() {
        let mut graph = InteractionGraph::default();
        graph.record(
            "/",
            Transition {
                action: "login".into(),
                target: "@login".into(),
                resulting_state: "/login".into(),
            },
        );
        assert_eq!(graph.shortest_path("/", "/login").unwrap().len(), 1);
    }

    #[test]
    fn unsafe_discovery_is_skipped_fail_closed() {
        let mut graph = InteractionGraph::default();
        let error = graph
            .record_live(
                LiveTransition {
                    action: InteractionActionKind::Click,
                    target: "@e1".into(),
                    safety: SafetyClass::Unknown,
                    pre_state: state("/", 1, "a"),
                    resulting_state: state("/", 1, "b"),
                    evidence_refs: vec![],
                },
                DiscoveryBounds::default(),
            )
            .unwrap_err();
        assert_eq!(error, GraphAdmissionError::UnsafeAction);
    }

    #[test]
    fn graph_hard_caps_edges() {
        let mut graph = InteractionGraph::default();
        let bounds = DiscoveryBounds {
            max_nodes: 8,
            max_edges: 1,
            max_route_states: 2,
            deadline_ms: 100,
        };
        graph
            .record_live(
                LiveTransition {
                    action: InteractionActionKind::Click,
                    target: "@e1".into(),
                    safety: SafetyClass::ExplicitlySafe,
                    pre_state: state("/", 1, "a"),
                    resulting_state: state("/", 1, "b"),
                    evidence_refs: vec![],
                },
                bounds,
            )
            .unwrap();
        let error = graph
            .record_live(
                LiveTransition {
                    action: InteractionActionKind::Click,
                    target: "@e2".into(),
                    safety: SafetyClass::ExplicitlySafe,
                    pre_state: state("/", 1, "b"),
                    resulting_state: state("/", 1, "c"),
                    evidence_refs: vec![],
                },
                bounds,
            )
            .unwrap_err();
        assert_eq!(error, GraphAdmissionError::EdgeBudgetExceeded);
    }

    #[test]
    fn route_drift_stops_keyboard_journey() {
        let initial = FocusObservation {
            transition_index: 0,
            reference: None,
            route: "/".into(),
            document_generation: 2,
            tabindex: None,
            hidden_or_offscreen: false,
            is_body_or_document: true,
        };
        let result = analyze_keyboard_journey(
            initial,
            vec![FocusObservation {
                transition_index: 1,
                reference: Some("@e1".into()),
                route: "/other".into(),
                document_generation: 2,
                tabindex: Some(0),
                hidden_or_offscreen: false,
                is_body_or_document: false,
            }],
            64,
        );
        assert!(!result.complete);
        assert_eq!(result.stopped_reason.as_deref(), Some("route_drift"));
    }

    #[test]
    fn repeated_single_focus_is_trap_evidence() {
        let initial = FocusObservation {
            transition_index: 0,
            reference: Some("@e1".into()),
            route: "/".into(),
            document_generation: 1,
            tabindex: Some(0),
            hidden_or_offscreen: false,
            is_body_or_document: false,
        };
        let transitions = (1..=4)
            .map(|index| FocusObservation {
                transition_index: index,
                reference: Some("@e1".into()),
                route: "/".into(),
                document_generation: 1,
                tabindex: Some(0),
                hidden_or_offscreen: false,
                is_body_or_document: false,
            })
            .collect();
        let result = analyze_keyboard_journey(initial, transitions, 64);
        assert!(result.issues.iter().any(|issue| issue.kind == FocusIssueKind::FocusTrap));
    }

    #[test]
    fn missing_feedback_is_not_promoted_to_dead_click() {
        let receipt = classify_feedback(
            true,
            200,
            800,
            250,
            FeedbackSignals::default(),
        );
        assert_eq!(receipt.verdict, FeedbackVerdict::NoObservedFeedback);
    }

    #[test]
    fn delayed_feedback_is_distinct() {
        let receipt = classify_feedback(
            true,
            400,
            800,
            250,
            FeedbackSignals {
                semantic_changed: true,
                ..FeedbackSignals::default()
            },
        );
        assert_eq!(receipt.verdict, FeedbackVerdict::DelayedFeedback);
    }

    #[test]
    fn replay_fails_on_state_mismatch_without_repair_guess() {
        let plan = vec![ReplayStep {
            index: 0,
            action: InteractionActionKind::Click,
            target: "@e1".into(),
            expected_before: state("/", 3, "a"),
            expected_after: state("/", 3, "b"),
            evidence_refs: vec!["evidence-1".into()],
        }];
        let receipt = replay_receipt(
            &plan,
            &[state("/", 3, "DIFFERENT"), state("/", 3, "b")],
            &[true],
        );
        assert_eq!(receipt.status, ReplayStatus::Failed);
        assert_eq!(receipt.reason.as_deref(), Some("pre_state_mismatch"));
        assert_eq!(receipt.first_failed_step, Some(0));
    }

    #[test]
    fn replay_fails_on_stable_ref_invalidation() {
        let plan = vec![ReplayStep {
            index: 0,
            action: InteractionActionKind::Click,
            target: "@e1".into(),
            expected_before: state("/", 3, "a"),
            expected_after: state("/", 3, "b"),
            evidence_refs: vec![],
        }];
        let receipt = replay_receipt(
            &plan,
            &[state("/", 3, "a"), state("/", 3, "b")],
            &[false],
        );
        assert_eq!(receipt.reason.as_deref(), Some("stable_ref_invalid"));
    }
}
