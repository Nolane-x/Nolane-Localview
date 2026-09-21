use std::collections::{BTreeMap, BTreeSet};

use localview_content_addressed::{ObjectHash, object_hash};
use localview_contracts::{ContractEvaluationSummary, ContractStrength, ContractVerdict};
use localview_counterfactual::{IsolationLevel, ShadowCandidateProof, ShadowCleanupProof};
use localview_mutation::{MutationChallengeResult, MutationVerdict};
use localview_planner::PartialRevalidationPlan;
use localview_state_space::AffectedStatePlan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ImpactKind {
    Route,
    Region,
    Reference,
    Contract,
    IssueClass,
    VisualRegion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImpactTarget {
    pub kind: ImpactKind,
    pub id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PredictedImpact {
    pub targets: BTreeSet<ImpactTarget>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActualImpact {
    pub targets: BTreeSet<ImpactTarget>,
    pub evidence_ids: Vec<String>,
    pub observation_scope_complete: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactComparison {
    pub predicted_and_observed: Vec<ImpactTarget>,
    pub predicted_but_not_observed: Vec<ImpactTarget>,
    pub unexpected_observed_impact: Vec<ImpactTarget>,
    pub inconclusive: Vec<ImpactTarget>,
    pub causal_claim: String,
}

pub fn compare_predicted_actual(
    predicted: &PredictedImpact,
    actual: &ActualImpact,
) -> ImpactComparison {
    let predicted_and_observed = predicted
        .targets
        .intersection(&actual.targets)
        .cloned()
        .collect::<Vec<_>>();
    let unexpected_observed_impact = actual
        .targets
        .difference(&predicted.targets)
        .cloned()
        .collect::<Vec<_>>();
    let missing = predicted
        .targets
        .difference(&actual.targets)
        .cloned()
        .collect::<Vec<_>>();
    let (predicted_but_not_observed, inconclusive) = if actual.observation_scope_complete {
        (missing, Vec::new())
    } else {
        (Vec::new(), missing)
    };
    ImpactComparison {
        predicted_and_observed,
        predicted_but_not_observed,
        unexpected_observed_impact,
        inconclusive,
        causal_claim:
            "impact comparison records observed correlation only; it does not establish root cause"
                .into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkippedState {
    pub state_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationResourceBudget {
    pub admitted: bool,
    pub max_states: usize,
    pub executed_states: usize,
    pub max_mutations: usize,
    pub executed_mutations: usize,
    pub max_runtime_ms: u64,
    pub observed_runtime_ms: u64,
    pub denial_reason: Option<String>,
}

impl VerificationResourceBudget {
    pub fn within_budget(&self) -> bool {
        self.admitted
            && self.executed_states <= self.max_states
            && self.executed_mutations <= self.max_mutations
            && self.observed_runtime_ms <= self.max_runtime_ms
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationCleanupProof {
    pub shadow: ShadowCleanupProof,
    pub shadow_processes_terminated: bool,
    pub loopback_port_released: bool,
    pub real_worktree_unchanged: bool,
    pub external_side_effects_observed: bool,
}

impl VerificationCleanupProof {
    pub fn complete(&self) -> bool {
        self.shadow.attempted
            && self.shadow.worktree_removed
            && self.shadow.directory_absent
            && self.shadow_processes_terminated
            && self.loopback_port_released
            && self.real_worktree_unchanged
            && !self.external_side_effects_observed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomousVerificationVerdict {
    Verified,
    Rejected,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomousVerificationReceipt {
    pub schema_version: u32,
    pub base_revision: String,
    pub candidate_id: String,
    pub patch_digest: String,
    pub isolation_type: IsolationLevel,
    pub affected_state_plan_hash: ObjectHash,
    pub contracts_evaluated: ContractEvaluationSummary,
    pub mutation_results: Vec<MutationChallengeResult>,
    pub predicted_impact: PredictedImpact,
    pub actual_impact: ActualImpact,
    pub impact_comparison: ImpactComparison,
    pub unexpected_impact: Vec<ImpactTarget>,
    pub evidence_ids: Vec<String>,
    pub stale_evidence_ids: Vec<String>,
    pub revalidated_state_set: Vec<String>,
    pub skipped_states: Vec<SkippedState>,
    pub resource_budget: VerificationResourceBudget,
    pub cleanup_proof: VerificationCleanupProof,
    pub final_verdict: AutonomousVerificationVerdict,
    pub reasons: Vec<String>,
}

impl AutonomousVerificationReceipt {
    pub fn digest(&self) -> ObjectHash {
        object_hash(self)
    }

    pub fn hard_contract_failures(&self) -> Vec<String> {
        self.contracts_evaluated
            .evaluated
            .iter()
            .filter(|record| {
                record.strength == ContractStrength::Hard && record.verdict == ContractVerdict::Fail
            })
            .map(|record| record.contract_id.clone())
            .collect()
    }

    pub fn surviving_mutations(&self) -> Vec<String> {
        self.mutation_results
            .iter()
            .filter(|result| result.outcome.verdict == MutationVerdict::Survived)
            .map(|result| result.outcome.mutation_id.to_string())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct AutonomousVerificationInput {
    pub affected: AffectedStatePlan,
    pub shadow_proof: ShadowCandidateProof,
    pub contracts: ContractEvaluationSummary,
    pub mutations: Vec<MutationChallengeResult>,
    pub predicted_impact: PredictedImpact,
    pub actual_impact: ActualImpact,
    pub revalidation_plan: PartialRevalidationPlan,
    pub revalidated_states: BTreeSet<String>,
    pub skipped_states: Vec<SkippedState>,
    pub stale_evidence_ids: Vec<String>,
    pub resource_budget: VerificationResourceBudget,
    pub cleanup_proof: VerificationCleanupProof,
}

pub fn build_autonomous_receipt(
    input: AutonomousVerificationInput,
) -> AutonomousVerificationReceipt {
    let impact_comparison = compare_predicted_actual(&input.predicted_impact, &input.actual_impact);
    let affected_state_plan_hash = object_hash(&input.affected);
    let mut reasons = Vec::new();

    let identity_mismatch = input.shadow_proof.base_revision != input.affected.base_revision
        || input.shadow_proof.patch_digest != input.affected.change.patch_digest
        || input.shadow_proof.candidate_id.to_string() != input.affected.change.candidate_id;
    if identity_mismatch {
        reasons.push("candidate identity/base revision/patch binding mismatch".into());
    }
    if !input.shadow_proof.real_worktree_unchanged {
        reasons.push("real working tree changed during candidate verification".into());
    }

    if !input.contracts.hard_failures.is_empty() {
        reasons.push(format!(
            "{} hard contract(s) failed",
            input.contracts.hard_failures.len()
        ));
    }
    if !input.contracts.hard_unknowns.is_empty() {
        reasons.push(format!(
            "{} hard contract(s) remain unknown",
            input.contracts.hard_unknowns.len()
        ));
    }

    let survived = input
        .mutations
        .iter()
        .filter(|result| result.outcome.verdict == MutationVerdict::Survived)
        .count();
    let skipped_or_invalid = input
        .mutations
        .iter()
        .filter(|result| {
            matches!(
                result.outcome.verdict,
                MutationVerdict::SkippedUnsafe | MutationVerdict::Invalid
            )
        })
        .count();
    if survived > 0 {
        reasons.push(format!("{survived} mutation challenge(s) survived"));
    }
    if skipped_or_invalid > 0 {
        reasons.push(format!(
            "{skipped_or_invalid} mutation challenge(s) were skipped or invalid"
        ));
    }

    if !impact_comparison.unexpected_observed_impact.is_empty() {
        reasons.push(format!(
            "{} unexpected actual impact target(s) were observed",
            impact_comparison.unexpected_observed_impact.len()
        ));
    }
    if !impact_comparison.inconclusive.is_empty() {
        reasons.push(format!(
            "{} predicted impact target(s) remain inconclusive",
            impact_comparison.inconclusive.len()
        ));
    }

    if !input.revalidation_plan.complete_claim_allowed {
        reasons.push("partial revalidation cannot claim complete coverage".into());
    }
    if !input.affected.denominator_known {
        reasons.push("affected-state denominator is unknown".into());
    }
    if input.affected.incomplete {
        reasons.push("affected-state plan is incomplete".into());
    }
    if !input.stale_evidence_ids.is_empty() {
        reasons.push(format!(
            "{} stale evidence object(s) were excluded",
            input.stale_evidence_ids.len()
        ));
    }
    if !input.resource_budget.within_budget() {
        reasons.push(
            input
                .resource_budget
                .denial_reason
                .clone()
                .unwrap_or_else(|| "verification resource budget was not satisfied".into()),
        );
    }
    if !input.cleanup_proof.complete() {
        reasons.push("candidate cleanup/isolation proof is incomplete".into());
    }

    let planned_states = input
        .revalidation_plan
        .state_keys
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let explicitly_skipped = input
        .skipped_states
        .iter()
        .map(|state| state.state_key.clone())
        .collect::<BTreeSet<_>>();
    let unaccounted = planned_states
        .difference(&input.revalidated_states)
        .filter(|state| !explicitly_skipped.contains(*state))
        .cloned()
        .collect::<Vec<_>>();
    if !unaccounted.is_empty() {
        reasons.push(format!(
            "{} planned revalidation state(s) were neither executed nor explicitly skipped",
            unaccounted.len()
        ));
    }

    let rejected = identity_mismatch
        || !input.shadow_proof.real_worktree_unchanged
        || !input.contracts.hard_failures.is_empty()
        || !input.cleanup_proof.complete();
    let inconclusive = !input.contracts.hard_unknowns.is_empty()
        || survived > 0
        || skipped_or_invalid > 0
        || !impact_comparison.unexpected_observed_impact.is_empty()
        || !impact_comparison.inconclusive.is_empty()
        || !input.revalidation_plan.complete_claim_allowed
        || !input.affected.denominator_known
        || input.affected.incomplete
        || !input.stale_evidence_ids.is_empty()
        || !input.resource_budget.within_budget()
        || !unaccounted.is_empty();

    let final_verdict = if rejected {
        AutonomousVerificationVerdict::Rejected
    } else if inconclusive {
        AutonomousVerificationVerdict::Inconclusive
    } else {
        AutonomousVerificationVerdict::Verified
    };

    let evidence_ids = collect_evidence_ids(&input);

    AutonomousVerificationReceipt {
        schema_version: 1,
        base_revision: input.affected.base_revision.clone(),
        candidate_id: input.affected.change.candidate_id.clone(),
        patch_digest: input.affected.change.patch_digest.clone(),
        isolation_type: input.shadow_proof.isolation,
        affected_state_plan_hash,
        contracts_evaluated: input.contracts,
        mutation_results: input.mutations,
        predicted_impact: input.predicted_impact,
        actual_impact: input.actual_impact,
        unexpected_impact: impact_comparison.unexpected_observed_impact.clone(),
        impact_comparison,
        evidence_ids,
        stale_evidence_ids: sorted_dedup(input.stale_evidence_ids),
        revalidated_state_set: input.revalidated_states.into_iter().collect(),
        skipped_states: input.skipped_states,
        resource_budget: input.resource_budget,
        cleanup_proof: input.cleanup_proof,
        final_verdict,
        reasons: sorted_dedup(reasons),
    }
}

fn collect_evidence_ids(input: &AutonomousVerificationInput) -> Vec<String> {
    let mut ids = BTreeSet::new();
    ids.extend(input.affected.evidence_provenance.iter().cloned());
    ids.extend(
        input
            .shadow_proof
            .changed_files
            .iter()
            .map(|path| format!("shadow-file:{path}")),
    );
    for record in &input.contracts.evaluated {
        ids.extend(record.evidence_ids.iter().cloned());
    }
    for result in &input.mutations {
        ids.extend(result.outcome.evidence_ids.iter().cloned());
    }
    ids.extend(input.predicted_impact.evidence_ids.iter().cloned());
    ids.extend(input.actual_impact.evidence_ids.iter().cloned());
    ids.into_iter().collect()
}

fn sorted_dedup(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn impact_targets_from_affected(plan: &AffectedStatePlan) -> PredictedImpact {
    let mut targets = BTreeSet::new();
    targets.extend(plan.impacted_routes.iter().cloned().map(|id| ImpactTarget {
        kind: ImpactKind::Route,
        id,
    }));
    targets.extend(
        plan.impacted_regions
            .iter()
            .cloned()
            .map(|id| ImpactTarget {
                kind: ImpactKind::Region,
                id,
            }),
    );
    targets.extend(plan.impacted_refs.iter().cloned().map(|id| ImpactTarget {
        kind: ImpactKind::Reference,
        id,
    }));
    targets.extend(
        plan.impacted_contracts
            .iter()
            .cloned()
            .map(|id| ImpactTarget {
                kind: ImpactKind::Contract,
                id,
            }),
    );
    PredictedImpact {
        targets,
        evidence_ids: plan.evidence_provenance.clone(),
    }
}

pub fn contract_verdict_counts(summary: &ContractEvaluationSummary) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for record in &summary.evaluated {
        let key = match record.verdict {
            ContractVerdict::Pass => "pass",
            ContractVerdict::Fail => "fail",
            ContractVerdict::Excepted => "excepted",
            ContractVerdict::Unknown => "unknown",
        };
        *counts.entry(key.into()).or_insert(0) += 1;
    }
    counts
}

pub fn record_native_postcondition(
    summary: &mut ContractEvaluationSummary,
    contract_id: impl Into<String>,
    strength: ContractStrength,
    evaluation: localview_postcondition_contracts::NativeSemanticPostconditionEvaluation,
    evidence_ids: Vec<String>,
) {
    let contract_id = contract_id.into();
    let verdict = match evaluation {
        localview_postcondition_contracts::NativeSemanticPostconditionEvaluation::VerifiedPass => {
            ContractVerdict::Pass
        }
        localview_postcondition_contracts::NativeSemanticPostconditionEvaluation::VerifiedFail => {
            ContractVerdict::Fail
        }
        localview_postcondition_contracts::NativeSemanticPostconditionEvaluation::Unknown => {
            ContractVerdict::Unknown
        }
    };
    let explanation = match verdict {
        ContractVerdict::Pass => "registered postcondition verified".to_string(),
        ContractVerdict::Fail => "registered postcondition failed".to_string(),
        ContractVerdict::Unknown => "registered postcondition is unknown".to_string(),
        ContractVerdict::Excepted => {
            unreachable!("postcondition adapter does not create exceptions")
        }
    };
    summary
        .evaluated
        .push(localview_contracts::ContractEvaluationRecord {
            contract_id: contract_id.clone(),
            strength,
            verdict,
            explanation,
            evidence_ids,
        });
    match (strength, verdict) {
        (_, ContractVerdict::Pass) => summary.pass_count += 1,
        (_, ContractVerdict::Excepted) => summary.excepted_count += 1,
        (ContractStrength::Hard, ContractVerdict::Fail) => summary.hard_failures.push(contract_id),
        (ContractStrength::Hard, ContractVerdict::Unknown) => {
            summary.hard_unknowns.push(contract_id)
        }
        (ContractStrength::Soft, ContractVerdict::Fail | ContractVerdict::Unknown) => {
            summary.soft_warnings.push(contract_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use localview_contracts::{
        ContractEvaluationRecord, ContractEvaluationSummary, ContractStrength, ContractVerdict,
    };
    use localview_counterfactual::{ShadowCandidateProof, ShadowCleanupProof};
    use localview_mutation::{
        MutationChallengeResult, MutationExecutionEvidence, MutationOutcome, MutationVerdict,
    };
    use localview_planner::{PartialRevalidationPlan, RevalidationMode};
    use localview_state_space::{
        AffectedChangeIdentity, AffectedStatePlan, ProductState, StateDimension,
    };
    use uuid::Uuid;

    use super::*;

    fn affected(candidate: Uuid) -> AffectedStatePlan {
        AffectedStatePlan {
            base_revision: "abc".into(),
            change: AffectedChangeIdentity {
                candidate_id: candidate.to_string(),
                patch_digest: "sha256:patch".into(),
                changed_project_files: vec!["src/App.tsx".into()],
            },
            impacted_routes: vec!["/".into()],
            impacted_regions: vec!["hero".into()],
            impacted_refs: vec!["@e1".into()],
            impacted_contracts: vec!["hard".into()],
            state_dimensions: Vec::<StateDimension>::new(),
            compiled_states: vec![ProductState {
                values: BTreeMap::from([("viewport".into(), "desktop".into())]),
                score: 1.0,
                provenance: vec!["test".into()],
            }],
            risk_score: 1.0,
            evidence_provenance: vec!["ev-impact".into()],
            denominator_known: true,
            eligible_state_count: Some(1),
            pair_coverage: Some(1.0),
            truncated: false,
            incomplete: false,
            incomplete_reasons: vec![],
        }
    }

    fn shadow(candidate: Uuid) -> ShadowCandidateProof {
        ShadowCandidateProof {
            candidate_id: candidate,
            base_revision: "abc".into(),
            patch_digest: "sha256:patch".into(),
            isolation: IsolationLevel::SemanticOnly,
            changed_files: vec!["src/App.tsx".into()],
            shadow_path: "/tmp/shadow".into(),
            original_worktree_dirty: true,
            real_worktree_unchanged: true,
            external_side_effects_blocked: true,
        }
    }

    fn contracts(
        verdict: ContractVerdict,
        strength: ContractStrength,
    ) -> ContractEvaluationSummary {
        let record = ContractEvaluationRecord {
            contract_id: if strength == ContractStrength::Hard {
                "hard".into()
            } else {
                "soft".into()
            },
            strength,
            verdict,
            explanation: "test".into(),
            evidence_ids: vec!["ev-contract".into()],
        };
        let mut summary = ContractEvaluationSummary {
            evaluated: vec![record],
            ..Default::default()
        };
        match (strength, verdict) {
            (_, ContractVerdict::Pass) => summary.pass_count = 1,
            (_, ContractVerdict::Excepted) => summary.excepted_count = 1,
            (ContractStrength::Hard, ContractVerdict::Fail) => {
                summary.hard_failures.push("hard".into())
            }
            (ContractStrength::Hard, ContractVerdict::Unknown) => {
                summary.hard_unknowns.push("hard".into())
            }
            (ContractStrength::Soft, ContractVerdict::Fail | ContractVerdict::Unknown) => {
                summary.soft_warnings.push("soft".into())
            }
        }
        summary
    }

    fn revalidation(complete: bool) -> PartialRevalidationPlan {
        PartialRevalidationPlan {
            mode: RevalidationMode::Partial,
            routes: vec!["/".into()],
            regions: vec!["hero".into()],
            refs: vec!["@e1".into()],
            contracts: vec!["hard".into()],
            responsive_widths: vec!["desktop".into()],
            flow_checkpoints: vec![],
            visual_baselines: vec![],
            source_semantic_checks: vec!["source".into()],
            state_keys: vec!["viewport=desktop".into()],
            escalation_reasons: vec![],
            denominator_known: complete,
            complete_claim_allowed: complete,
        }
    }

    fn cleanup(ok: bool) -> VerificationCleanupProof {
        VerificationCleanupProof {
            shadow: ShadowCleanupProof {
                attempted: true,
                worktree_removed: ok,
                directory_absent: ok,
            },
            shadow_processes_terminated: ok,
            loopback_port_released: ok,
            real_worktree_unchanged: ok,
            external_side_effects_observed: false,
        }
    }

    fn budget(admitted: bool) -> VerificationResourceBudget {
        VerificationResourceBudget {
            admitted,
            max_states: 4,
            executed_states: 1,
            max_mutations: 4,
            executed_mutations: 0,
            max_runtime_ms: 10_000,
            observed_runtime_ms: 100,
            denial_reason: (!admitted).then_some("governor denied".into()),
        }
    }

    fn input(
        candidate: Uuid,
        contract_summary: ContractEvaluationSummary,
    ) -> AutonomousVerificationInput {
        let affected = affected(candidate);
        let predicted = impact_targets_from_affected(&affected);
        AutonomousVerificationInput {
            affected,
            shadow_proof: shadow(candidate),
            contracts: contract_summary,
            mutations: vec![],
            actual_impact: ActualImpact {
                targets: predicted.targets.clone(),
                evidence_ids: vec!["ev-actual".into()],
                observation_scope_complete: true,
            },
            predicted_impact: predicted,
            revalidation_plan: revalidation(true),
            revalidated_states: BTreeSet::from(["viewport=desktop".into()]),
            skipped_states: vec![],
            stale_evidence_ids: vec![],
            resource_budget: budget(true),
            cleanup_proof: cleanup(true),
        }
    }

    #[test]
    fn impact_comparison_separates_predicted_only_unexpected_and_inconclusive() {
        let predicted = PredictedImpact {
            targets: BTreeSet::from([
                ImpactTarget {
                    kind: ImpactKind::Route,
                    id: "/a".into(),
                },
                ImpactTarget {
                    kind: ImpactKind::Region,
                    id: "hero".into(),
                },
            ]),
            evidence_ids: vec![],
        };
        let actual = ActualImpact {
            targets: BTreeSet::from([
                ImpactTarget {
                    kind: ImpactKind::Route,
                    id: "/a".into(),
                },
                ImpactTarget {
                    kind: ImpactKind::Region,
                    id: "footer".into(),
                },
            ]),
            evidence_ids: vec![],
            observation_scope_complete: true,
        };
        let comparison = compare_predicted_actual(&predicted, &actual);
        assert_eq!(comparison.predicted_and_observed.len(), 1);
        assert_eq!(comparison.predicted_but_not_observed.len(), 1);
        assert_eq!(comparison.unexpected_observed_impact.len(), 1);
        assert!(comparison.inconclusive.is_empty());

        let incomplete = compare_predicted_actual(
            &predicted,
            &ActualImpact {
                observation_scope_complete: false,
                ..actual
            },
        );
        assert!(incomplete.predicted_but_not_observed.is_empty());
        assert_eq!(incomplete.inconclusive.len(), 1);
    }

    #[test]
    fn route_drift_is_recorded_as_unexpected_actual_impact() {
        let predicted = PredictedImpact {
            targets: BTreeSet::from([ImpactTarget {
                kind: ImpactKind::Route,
                id: "/expected".into(),
            }]),
            evidence_ids: vec!["ev-predicted".into()],
        };
        let actual = ActualImpact {
            targets: BTreeSet::from([ImpactTarget {
                kind: ImpactKind::Route,
                id: "/unexpected".into(),
            }]),
            evidence_ids: vec!["ev-actual".into()],
            observation_scope_complete: true,
        };
        let comparison = compare_predicted_actual(&predicted, &actual);
        assert_eq!(
            comparison.unexpected_observed_impact,
            vec![ImpactTarget {
                kind: ImpactKind::Route,
                id: "/unexpected".into(),
            }]
        );
        assert_eq!(
            comparison.predicted_but_not_observed,
            vec![ImpactTarget {
                kind: ImpactKind::Route,
                id: "/expected".into(),
            }]
        );
    }

    #[test]
    fn hard_contract_failure_rejects_candidate() {
        let candidate = Uuid::new_v4();
        let receipt = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Fail, ContractStrength::Hard),
        ));
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Rejected
        );
    }

    #[test]
    fn hard_unknown_resource_denial_and_stale_evidence_are_inconclusive() {
        let candidate = Uuid::new_v4();
        let mut input = input(
            candidate,
            contracts(ContractVerdict::Unknown, ContractStrength::Hard),
        );
        input.resource_budget = budget(false);
        input.stale_evidence_ids.push("old".into());
        let receipt = build_autonomous_receipt(input);
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Inconclusive
        );
    }

    #[test]
    fn unexpected_impact_and_surviving_mutation_are_inconclusive() {
        let candidate = Uuid::new_v4();
        let mut input = input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        );
        input.actual_impact.targets.insert(ImpactTarget {
            kind: ImpactKind::Region,
            id: "unexpected".into(),
        });
        let mutation_id = Uuid::new_v4();
        input.mutations.push(MutationChallengeResult {
            outcome: MutationOutcome {
                mutation_id,
                verdict: MutationVerdict::Survived,
                triggered_detectors: BTreeSet::new(),
                evidence_ids: vec!["mutation:1".into()],
            },
            execution: MutationExecutionEvidence {
                mutation_id,
                safety_checked: true,
                applied_in_isolation: true,
                external_side_effects: false,
                before_digest: "before".into(),
                after_digest: Some("after".into()),
                triggered_detectors: BTreeSet::new(),
                reason: "survived".into(),
                evidence_id: "mutation:1".into(),
            },
        });
        let receipt = build_autonomous_receipt(input);
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Inconclusive
        );
        assert_eq!(receipt.surviving_mutations().len(), 1);
        assert_eq!(receipt.unexpected_impact.len(), 1);
    }

    #[test]
    fn cleanup_failure_rejects_even_when_checks_pass() {
        let candidate = Uuid::new_v4();
        let mut input = input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        );
        input.cleanup_proof = cleanup(false);
        let receipt = build_autonomous_receipt(input);
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Rejected
        );
    }

    #[test]
    fn soft_warning_does_not_masquerade_as_hard_failure() {
        let candidate = Uuid::new_v4();
        let receipt = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Fail, ContractStrength::Soft),
        ));
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Verified
        );
        assert!(receipt.hard_contract_failures().is_empty());
        assert_eq!(receipt.contracts_evaluated.soft_warnings, vec!["soft"]);
    }

    #[test]
    fn complete_clean_proof_is_verified_and_serializable() {
        let candidate = Uuid::new_v4();
        let receipt = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        ));
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Verified
        );
        let json = serde_json::to_string(&receipt).unwrap();
        assert!(json.contains("\"final_verdict\":\"verified\""));
        assert!(receipt.digest().starts_with("sha256:"));
    }
}
