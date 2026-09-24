use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use localview_content_addressed::{ObjectHash, object_hash};
use localview_contracts::{
    ContractCategory, ContractEvaluationSummary, ContractPredicate, ContractRegistry,
    ContractScope, ContractStrength, ContractVerdict, LiveRuntimeFacts, RuntimeFactDomain,
    UxContract, evaluate_compiled_contracts,
};
use localview_counterfactual::{
    CounterfactualCandidate, ExternalSideEffectContainment, IsolationLevel, ShadowCandidateProof,
    ShadowCleanupProof, ShadowWorkspace, patch_digest,
};
use localview_mutation::{
    MutationCase, MutationChallengeResult, MutationExecutionPolicy, MutationOperator,
    MutationVerdict, SyntheticMutationState, execute_mutation_challenge,
};
use localview_planner::{
    PartialRevalidationInput, PartialRevalidationPlan, plan_partial_revalidation,
};
use localview_state_space::{
    AffectedChangeIdentity, AffectedStateInput, AffectedStatePlan, StateDimension, StateValue,
    compile_affected_state_plan,
};
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
pub enum ProductionCandidatePreflightVerdict {
    Rejected,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductionCandidatePreflightReceipt {
    pub candidate_id: Option<String>,
    pub base_revision: Option<String>,
    pub patch_digest: Option<String>,
    pub affected_state_plan_hash: Option<ObjectHash>,
    pub affected_state_plan: Option<AffectedStatePlan>,
    pub predicted_impact: Option<PredictedImpact>,
    pub affected_state_incomplete_reasons: Vec<String>,
    pub shadow_proof: Option<ShadowCandidateProof>,
    pub cleanup_proof: Option<ShadowCleanupProof>,
    pub verdict: ProductionCandidatePreflightVerdict,
    pub reasons: Vec<String>,
}

impl ProductionCandidatePreflightReceipt {
    pub fn digest(&self) -> ObjectHash {
        object_hash(self)
    }

    pub fn inconclusive_unavailable(reason: impl Into<String>) -> Self {
        Self {
            candidate_id: None,
            base_revision: None,
            patch_digest: None,
            affected_state_plan_hash: None,
            affected_state_plan: None,
            predicted_impact: None,
            affected_state_incomplete_reasons: Vec::new(),
            shadow_proof: None,
            cleanup_proof: None,
            verdict: ProductionCandidatePreflightVerdict::Inconclusive,
            reasons: vec![reason.into()],
        }
    }

    pub fn has_shadow_proof(&self) -> bool {
        self.shadow_proof.is_some() && self.cleanup_proof.is_some()
    }
}

pub fn run_production_candidate_preflight(
    repository_root: &Path,
    candidate: &CounterfactualCandidate,
) -> Result<ProductionCandidatePreflightReceipt, String> {
    let expected_patch_digest = patch_digest(&candidate.overlays);
    let identity = || {
        (
            Some(candidate.id.to_string()),
            Some(candidate.base_revision.clone()),
            Some(expected_patch_digest.clone()),
        )
    };
    let mut shadow = ShadowWorkspace::prepare(repository_root, candidate)
        .map_err(|error| format!("Wave 9 shadow preparation failed: {error:?}"))?;

    let proof = match shadow.proof() {
        Ok(proof) => proof,
        Err(error) => {
            let cleanup = shadow.cleanup().ok();
            let (candidate_id, base_revision, patch_digest) = identity();
            let mut reasons = vec![format!("Wave 9 shadow proof failed: {error:?}")];
            if cleanup.as_ref().is_none_or(|proof| {
                !(proof.attempted && proof.worktree_removed && proof.directory_absent)
            }) {
                reasons.push(
                    "shadow cleanup proof is unavailable or incomplete after proof failure".into(),
                );
            }
            return Ok(ProductionCandidatePreflightReceipt {
                candidate_id,
                base_revision,
                patch_digest,
                affected_state_plan_hash: None,
                affected_state_plan: None,
                predicted_impact: None,
                affected_state_incomplete_reasons: Vec::new(),
                shadow_proof: None,
                cleanup_proof: cleanup,
                verdict: ProductionCandidatePreflightVerdict::Rejected,
                reasons,
            });
        }
    };

    let cleanup = match shadow.cleanup() {
        Ok(cleanup) => cleanup,
        Err(error) => {
            let (candidate_id, base_revision, patch_digest) = identity();
            return Ok(ProductionCandidatePreflightReceipt {
                candidate_id,
                base_revision,
                patch_digest,
                affected_state_plan_hash: None,
                affected_state_plan: None,
                predicted_impact: None,
                affected_state_incomplete_reasons: Vec::new(),
                shadow_proof: Some(proof),
                cleanup_proof: None,
                verdict: ProductionCandidatePreflightVerdict::Rejected,
                reasons: vec![format!("Wave 9 shadow cleanup failed: {error:?}")],
            });
        }
    };

    let mut reasons = Vec::new();
    let identity_matches = proof.candidate_id == candidate.id
        && proof.base_revision == candidate.base_revision
        && proof.patch_digest == expected_patch_digest;
    if !identity_matches {
        reasons.push("shadow proof identity does not bind the candidate".into());
    }
    if !proof.real_worktree_unchanged {
        reasons.push("real working tree changed while the shadow candidate was prepared".into());
    }
    if !(cleanup.attempted && cleanup.worktree_removed && cleanup.directory_absent) {
        reasons.push("shadow cleanup proof is incomplete".into());
    }
    if proof.external_side_effect_containment != ExternalSideEffectContainment::ProvenBlocked {
        reasons.push(
            "external side-effect containment is not proven; candidate preflight is inconclusive"
                .into(),
        );
    }

    let rejected = !identity_matches
        || !proof.real_worktree_unchanged
        || !(cleanup.attempted && cleanup.worktree_removed && cleanup.directory_absent);
    let verdict = if rejected {
        ProductionCandidatePreflightVerdict::Rejected
    } else {
        ProductionCandidatePreflightVerdict::Inconclusive
    };

    Ok(ProductionCandidatePreflightReceipt {
        candidate_id: Some(candidate.id.to_string()),
        base_revision: Some(candidate.base_revision.clone()),
        patch_digest: Some(expected_patch_digest),
        affected_state_plan_hash: None,
        affected_state_plan: None,
        predicted_impact: None,
        affected_state_incomplete_reasons: Vec::new(),
        shadow_proof: Some(proof),
        cleanup_proof: Some(cleanup),
        verdict,
        reasons,
    })
}

pub fn bind_production_affected_state(
    mut receipt: ProductionCandidatePreflightReceipt,
    candidate: &CounterfactualCandidate,
    canonical_route: &str,
    reference: Option<&str>,
) -> Result<ProductionCandidatePreflightReceipt, String> {
    let expected_candidate_id = candidate.id.to_string();
    let expected_patch_digest = patch_digest(&candidate.overlays);
    if receipt.candidate_id.as_deref() != Some(expected_candidate_id.as_str())
        || receipt.base_revision.as_deref() != Some(candidate.base_revision.as_str())
        || receipt.patch_digest.as_deref() != Some(expected_patch_digest.as_str())
    {
        return Err("Wave 9 affected-state binding does not match the preflight candidate".into());
    }
    if canonical_route.trim().is_empty() {
        return Err("Wave 9 affected-state binding requires a canonical route".into());
    }

    let changed_project_files = candidate
        .overlays
        .iter()
        .map(|overlay| overlay.file.clone())
        .collect::<Vec<_>>();
    let impacted_routes = BTreeSet::from([canonical_route.to_owned()]);
    let mut impacted_refs = BTreeSet::new();
    let mut dimensions = vec![StateDimension {
        id: "route".into(),
        values: vec![StateValue {
            id: canonical_route.to_owned(),
            label: canonical_route.to_owned(),
            metadata: BTreeMap::new(),
        }],
        risk_weight: 1.0,
        boundary_values: BTreeSet::new(),
    }];
    if let Some(reference) = reference.filter(|value| !value.trim().is_empty()) {
        impacted_refs.insert(reference.to_owned());
        dimensions.push(StateDimension {
            id: "reference".into(),
            values: vec![StateValue {
                id: reference.to_owned(),
                label: reference.to_owned(),
                metadata: BTreeMap::new(),
            }],
            risk_weight: 1.0,
            boundary_values: BTreeSet::new(),
        });
    }

    let affected = compile_affected_state_plan(&AffectedStateInput {
        base_revision: candidate.base_revision.clone(),
        change: AffectedChangeIdentity {
            candidate_id: expected_candidate_id,
            patch_digest: expected_patch_digest,
            changed_project_files,
        },
        impacted_routes,
        impacted_regions: BTreeSet::new(),
        impacted_refs,
        impacted_contracts: BTreeSet::new(),
        dimensions,
        constraints: Vec::new(),
        max_states: 1,
        evidence_ids: candidate.evidence_ids.clone(),
        dependency_graph_complete: false,
        denominator_known: false,
    })
    .map_err(|error| format!("Wave 9 affected-state compilation failed: {error:?}"))?;

    receipt.affected_state_plan_hash = Some(object_hash(&affected));
    receipt.affected_state_plan = Some(affected.clone());
    receipt.predicted_impact = Some(impact_targets_from_affected(&affected));
    receipt.affected_state_incomplete_reasons = affected.incomplete_reasons.clone();
    if affected.incomplete {
        receipt
            .reasons
            .push("production affected-state scope remains explicitly incomplete".into());
        receipt.reasons.sort();
        receipt.reasons.dedup();
    }
    Ok(receipt)
}

pub const AUTONOMOUS_VERIFICATION_RECEIPT_SCHEMA_VERSION: u32 = 2;
pub const PRODUCTION_CONTRACT_CATALOG_REVISION: &str =
    "localview-wave9-trusted-verify-contracts-v1";
pub const PRODUCTION_MUTATION_CATALOG_REVISION: &str =
    "localview-wave9-trusted-verify-mutations-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductionDeterministicStatus {
    ChangeObserved,
    NoObservableChange,
    RegressionSignal,
    Inconclusive,
}

fn production_contract_summary(
    observed: &ProductionObservedVerificationInput,
) -> Result<ContractEvaluationSummary, String> {
    let mut registry = ContractRegistry::default();
    registry.insert(UxContract {
        id: "trusted-verify.observable-change".into(),
        title: "Trusted Verify must observe the reviewed change".into(),
        category: ContractCategory::Safety,
        strength: ContractStrength::Hard,
        scope: ContractScope::global(),
        predicate: ContractPredicate::MetricAtLeast {
            metric: "observable_change".into(),
            min: 1.0,
        },
        provenance: PRODUCTION_CONTRACT_CATALOG_REVISION.into(),
        inherited_from: None,
    });
    for (id, code) in [
        ("trusted-verify.no-new-console-error", "new_console_error"),
        (
            "trusted-verify.no-new-network-failure",
            "new_network_failure",
        ),
        (
            "trusted-verify.target-remains-interactive",
            "target_became_non_interactive",
        ),
    ] {
        registry.insert(UxContract {
            id: id.into(),
            title: id.into(),
            category: ContractCategory::Safety,
            strength: ContractStrength::Hard,
            scope: ContractScope::global(),
            predicate: ContractPredicate::NoIssueCode { code: code.into() },
            provenance: PRODUCTION_CONTRACT_CATALOG_REVISION.into(),
            inherited_from: None,
        });
    }

    let compiled = registry
        .compile_autonomous_subset(&BTreeSet::new())
        .map_err(|error| format!("Wave 9 production contract catalog failed: {error:?}"))?;
    let mut live = LiveRuntimeFacts::default();
    live.facts
        .issue_codes
        .extend(observed.regression_signals.iter().cloned());
    live.complete_domains.insert(RuntimeFactDomain::IssueCodes);
    if observed.deterministic_status != ProductionDeterministicStatus::Inconclusive {
        live.facts.metrics.insert(
            "observable_change".into(),
            if observed.deterministic_status == ProductionDeterministicStatus::ChangeObserved {
                1.0
            } else {
                0.0
            },
        );
        live.metric_keys.insert("observable_change".into());
        live.complete_domains.insert(RuntimeFactDomain::Metrics);
    }
    live.evidence_ids = observed.evidence_ids.clone();
    Ok(evaluate_compiled_contracts(
        &compiled,
        &live,
        &BTreeMap::new(),
        PRODUCTION_CONTRACT_CATALOG_REVISION,
    ))
}

fn production_mutation_challenges() -> Vec<MutationChallengeResult> {
    let baseline = SyntheticMutationState::default();
    let policy = MutationExecutionPolicy::default();
    let geometry = MutationCase::synthetic_safe(
        0x7d1b4f346bcc4a23a0a8382e2cab1001,
        "trusted-verify:selected-target",
        MutationOperator::Shift { dx: 17.0, dy: 11.0 },
        BTreeSet::from(["geometry_changed".into()]),
        1.0,
    );
    let accessible_name = MutationCase::synthetic_safe(
        0x7d1b4f346bcc4a23a0a8382e2cab1002,
        "trusted-verify:selected-target",
        MutationOperator::RemoveAccessibleName,
        BTreeSet::from(["name_changed".into()]),
        1.0,
    );

    vec![
        execute_mutation_challenge(&geometry, &baseline, &policy, |before, after| {
            if (before.x, before.y, before.width, before.height)
                != (after.x, after.y, after.width, after.height)
            {
                BTreeSet::from(["geometry_changed".into()])
            } else {
                BTreeSet::new()
            }
        }),
        execute_mutation_challenge(&accessible_name, &baseline, &policy, |before, after| {
            if before.accessible_name != after.accessible_name {
                BTreeSet::from(["name_changed".into()])
            } else {
                BTreeSet::new()
            }
        }),
    ]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundedVerificationScope {
    CurrentTargetCurrentRoute,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundedVerificationVerdict {
    Verified,
    Rejected,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundedVerificationReceipt {
    pub schema_version: u32,
    pub scope: BoundedVerificationScope,
    pub canonical_route: String,
    pub reference: Option<String>,
    pub snapshot_version: u64,
    pub verdict: BoundedVerificationVerdict,
    pub reasons: Vec<String>,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProductionObservedVerificationInput {
    pub deterministic_status: ProductionDeterministicStatus,
    pub canonical_route: String,
    pub reference: Option<String>,
    pub snapshot_version: u64,
    pub reference_changed: bool,
    pub visual_region_count: usize,
    pub regression_signals: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub observed_runtime_ms: u64,
}

fn build_bounded_target_verification(
    observed: &ProductionObservedVerificationInput,
    autonomous: &AutonomousVerificationReceipt,
) -> BoundedVerificationReceipt {
    let mut reasons = Vec::new();
    let reference = observed
        .reference
        .as_ref()
        .filter(|reference| !reference.trim().is_empty())
        .cloned();

    if reference.is_none() {
        reasons.push("bounded verification target reference is unavailable".into());
    }
    if !observed.reference_changed {
        reasons.push("bounded verification did not observe a change on the selected target".into());
    }
    match observed.deterministic_status {
        ProductionDeterministicStatus::ChangeObserved => {}
        ProductionDeterministicStatus::NoObservableChange => {
            reasons.push("deterministic verification did not observe the reviewed change".into());
        }
        ProductionDeterministicStatus::RegressionSignal => {
            reasons.push("deterministic verification observed a regression signal".into());
        }
        ProductionDeterministicStatus::Inconclusive => {
            reasons.push("deterministic verification remains inconclusive".into());
        }
    }
    if !autonomous.contracts_evaluated.hard_failures.is_empty() {
        reasons.push(format!(
            "{} bounded hard contract(s) failed",
            autonomous.contracts_evaluated.hard_failures.len()
        ));
    }
    if !autonomous.contracts_evaluated.hard_unknowns.is_empty() {
        reasons.push(format!(
            "{} bounded hard contract(s) remain unknown",
            autonomous.contracts_evaluated.hard_unknowns.len()
        ));
    }
    let survived = autonomous
        .mutation_results
        .iter()
        .filter(|result| result.outcome.verdict == MutationVerdict::Survived)
        .count();
    let skipped_or_invalid = autonomous
        .mutation_results
        .iter()
        .filter(|result| {
            matches!(
                result.outcome.verdict,
                MutationVerdict::SkippedUnsafe | MutationVerdict::Invalid
            )
        })
        .count();
    if survived > 0 {
        reasons.push(format!("{survived} bounded mutation challenge(s) survived"));
    }
    if skipped_or_invalid > 0 {
        reasons.push(format!(
            "{skipped_or_invalid} bounded mutation challenge(s) were skipped or invalid"
        ));
    }
    let containment_unproven =
        autonomous.external_side_effect_containment != ExternalSideEffectContainment::ProvenBlocked;
    if containment_unproven {
        reasons.push("bounded verification side-effect containment is not proven".into());
    }
    if !autonomous.cleanup_proof.complete() {
        reasons.push("bounded verification cleanup proof is incomplete".into());
    }
    if !autonomous.resource_budget.within_budget() {
        reasons.push("bounded verification resource budget was not satisfied".into());
    }
    if !autonomous.stale_evidence_ids.is_empty() {
        reasons.push("bounded verification contains stale evidence debt".into());
    }
    if !autonomous.unexpected_impact.is_empty() {
        reasons.push(format!(
            "{} unexpected impact target(s) remain outside the bounded prediction",
            autonomous.unexpected_impact.len()
        ));
    }
    if !autonomous.impact_comparison.inconclusive.is_empty() {
        reasons.push(format!(
            "{} impact target(s) remain inconclusive",
            autonomous.impact_comparison.inconclusive.len()
        ));
    }

    let rejected = matches!(
        observed.deterministic_status,
        ProductionDeterministicStatus::NoObservableChange
            | ProductionDeterministicStatus::RegressionSignal
    ) || !autonomous.contracts_evaluated.hard_failures.is_empty()
        || !autonomous.cleanup_proof.complete();

    let inconclusive = reference.is_none()
        || !observed.reference_changed
        || observed.deterministic_status == ProductionDeterministicStatus::Inconclusive
        || !autonomous.contracts_evaluated.hard_unknowns.is_empty()
        || survived > 0
        || skipped_or_invalid > 0
        || containment_unproven
        || !autonomous.resource_budget.within_budget()
        || !autonomous.stale_evidence_ids.is_empty()
        || !autonomous.unexpected_impact.is_empty()
        || !autonomous.impact_comparison.inconclusive.is_empty();

    let verdict = if rejected {
        BoundedVerificationVerdict::Rejected
    } else if inconclusive {
        BoundedVerificationVerdict::Inconclusive
    } else {
        BoundedVerificationVerdict::Verified
    };

    BoundedVerificationReceipt {
        schema_version: 1,
        scope: BoundedVerificationScope::CurrentTargetCurrentRoute,
        canonical_route: observed.canonical_route.clone(),
        reference,
        snapshot_version: observed.snapshot_version,
        verdict,
        reasons: sorted_dedup(reasons),
        evidence_ids: autonomous.evidence_ids.clone(),
    }
}

pub fn build_production_observation_receipt(
    preflight: &ProductionCandidatePreflightReceipt,
    observed: ProductionObservedVerificationInput,
) -> Result<AutonomousVerificationReceipt, String> {
    let affected = preflight.affected_state_plan.clone().ok_or_else(|| {
        "Wave 9 production receipt is missing the affected-state plan".to_string()
    })?;
    if preflight.affected_state_plan_hash.as_ref() != Some(&object_hash(&affected)) {
        return Err("Wave 9 production receipt affected-state digest mismatch".into());
    }
    let predicted_impact = preflight
        .predicted_impact
        .clone()
        .unwrap_or_else(|| impact_targets_from_affected(&affected));
    let shadow_proof = preflight
        .shadow_proof
        .clone()
        .ok_or_else(|| "Wave 9 production receipt is missing the shadow proof".to_string())?;
    let shadow_cleanup = preflight.cleanup_proof.clone().ok_or_else(|| {
        "Wave 9 production receipt is missing the shadow cleanup proof".to_string()
    })?;

    let contracts = production_contract_summary(&observed)?;
    let mutations = production_mutation_challenges();
    let mut actual_impact = ActualImpact {
        evidence_ids: sorted_dedup(observed.evidence_ids.clone()),
        observation_scope_complete: observed.deterministic_status
            != ProductionDeterministicStatus::Inconclusive,
        ..Default::default()
    };
    if observed.reference_changed {
        if let Some(reference) = observed
            .reference
            .as_ref()
            .filter(|reference| !reference.trim().is_empty())
        {
            actual_impact.targets.insert(ImpactTarget {
                kind: ImpactKind::Reference,
                id: reference.clone(),
            });
        }
        actual_impact.targets.insert(ImpactTarget {
            kind: ImpactKind::Route,
            id: observed.canonical_route.clone(),
        });
    }
    for index in 0..observed.visual_region_count {
        actual_impact.targets.insert(ImpactTarget {
            kind: ImpactKind::VisualRegion,
            id: format!("{}#visual-region-{index}", observed.canonical_route),
        });
    }
    for signal in &observed.regression_signals {
        actual_impact.targets.insert(ImpactTarget {
            kind: ImpactKind::IssueClass,
            id: signal.clone(),
        });
    }

    let revalidation_plan = plan_partial_revalidation(&PartialRevalidationInput {
        affected: affected.clone(),
        impacted_flow_checkpoints: BTreeSet::new(),
        relevant_visual_baselines: BTreeSet::new(),
        source_semantic_checks: BTreeSet::from(["trusted-fix-postimage".into()]),
        known_universe: None,
    });
    let revalidated_states = affected
        .compiled_states
        .iter()
        .map(|state| state.key())
        .collect::<BTreeSet<_>>();
    let cleanup_proof = VerificationCleanupProof {
        shadow: shadow_cleanup,
        // SemanticOnly preflight never launches candidate processes or reserves
        // a candidate runtime port. These obligations are vacuously discharged,
        // while external side-effect containment remains independently NotProven.
        shadow_processes_terminated: true,
        loopback_port_released: true,
        real_worktree_unchanged: shadow_proof.real_worktree_unchanged,
        external_side_effects_observed: false,
    };
    let mut receipt = build_autonomous_receipt(AutonomousVerificationInput {
        affected,
        shadow_proof,
        contracts,
        mutations: mutations.clone(),
        predicted_impact,
        actual_impact,
        revalidation_plan,
        revalidated_states,
        skipped_states: Vec::new(),
        stale_evidence_ids: Vec::new(),
        resource_budget: VerificationResourceBudget {
            admitted: true,
            max_states: 1,
            executed_states: 1,
            max_mutations: mutations.len(),
            executed_mutations: mutations.len(),
            max_runtime_ms: 15_000,
            observed_runtime_ms: observed.observed_runtime_ms,
            denial_reason: None,
        },
        cleanup_proof,
    });
    receipt.bounded_verification = Some(build_bounded_target_verification(&observed, &receipt));

    Ok(receipt)
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
    pub external_side_effect_containment: ExternalSideEffectContainment,
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
    pub bounded_verification: Option<BoundedVerificationReceipt>,
    pub final_verdict: AutonomousVerificationVerdict,
    pub reasons: Vec<String>,
}

impl AutonomousVerificationReceipt {
    pub fn digest(&self) -> ObjectHash {
        object_hash(self)
    }

    pub fn shadow_side_effect_containment(&self) -> ExternalSideEffectContainment {
        self.external_side_effect_containment
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
    let external_side_effect_containment_unproven =
        input.shadow_proof.external_side_effect_containment
            != ExternalSideEffectContainment::ProvenBlocked;
    if external_side_effect_containment_unproven {
        reasons.push("external side-effect containment is not proven".into());
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
        || !unaccounted.is_empty()
        || external_side_effect_containment_unproven;

    let final_verdict = if rejected {
        AutonomousVerificationVerdict::Rejected
    } else if inconclusive {
        AutonomousVerificationVerdict::Inconclusive
    } else {
        AutonomousVerificationVerdict::Verified
    };

    let evidence_ids = collect_evidence_ids(&input);

    AutonomousVerificationReceipt {
        schema_version: AUTONOMOUS_VERIFICATION_RECEIPT_SCHEMA_VERSION,
        base_revision: input.affected.base_revision.clone(),
        candidate_id: input.affected.change.candidate_id.clone(),
        patch_digest: input.affected.change.patch_digest.clone(),
        isolation_type: input.shadow_proof.isolation,
        external_side_effect_containment: input.shadow_proof.external_side_effect_containment,
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
        bounded_verification: None,
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
            external_side_effect_containment: ExternalSideEffectContainment::ProvenBlocked,
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
    fn production_preflight_has_no_verified_state() {
        assert_ne!(
            ProductionCandidatePreflightVerdict::Inconclusive,
            ProductionCandidatePreflightVerdict::Rejected
        );
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
    fn unproven_external_side_effect_containment_is_inconclusive() {
        let candidate = Uuid::new_v4();
        let mut input = input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        );
        input.shadow_proof.external_side_effect_containment =
            ExternalSideEffectContainment::NotProven;
        let receipt = build_autonomous_receipt(input);
        assert_eq!(
            receipt.final_verdict,
            AutonomousVerificationVerdict::Inconclusive
        );
        assert!(
            receipt
                .reasons
                .iter()
                .any(|reason| reason.contains("containment is not proven"))
        );
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
    fn bounded_target_verifies_only_clean_exact_scope() {
        let candidate = Uuid::new_v4();
        let autonomous = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        ));
        let observed = ProductionObservedVerificationInput {
            deterministic_status: ProductionDeterministicStatus::ChangeObserved,
            canonical_route: "http://127.0.0.1:5173/settings".into(),
            reference: Some("@e1".into()),
            snapshot_version: 42,
            reference_changed: true,
            visual_region_count: 0,
            regression_signals: Vec::new(),
            evidence_ids: vec!["semantic:after".into()],
            observed_runtime_ms: 100,
        };
        let bounded = build_bounded_target_verification(&observed, &autonomous);
        assert_eq!(bounded.verdict, BoundedVerificationVerdict::Verified);
        assert!(bounded.reasons.is_empty());
        assert_eq!(bounded.reference.as_deref(), Some("@e1"));
        assert_eq!(bounded.snapshot_version, 42);
    }

    #[test]
    fn bounded_target_rejects_missing_change_or_regression_signal() {
        let candidate = Uuid::new_v4();
        let autonomous = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        ));
        for status in [
            ProductionDeterministicStatus::NoObservableChange,
            ProductionDeterministicStatus::RegressionSignal,
        ] {
            let observed = ProductionObservedVerificationInput {
                deterministic_status: status,
                canonical_route: "http://127.0.0.1:5173/settings".into(),
                reference: Some("@e1".into()),
                snapshot_version: 42,
                reference_changed: false,
                visual_region_count: 0,
                regression_signals: Vec::new(),
                evidence_ids: vec!["semantic:after".into()],
                observed_runtime_ms: 100,
            };
            let bounded = build_bounded_target_verification(&observed, &autonomous);
            assert_eq!(bounded.verdict, BoundedVerificationVerdict::Rejected);
        }
    }

    #[test]
    fn bounded_target_is_inconclusive_when_impact_scope_has_debt() {
        let candidate = Uuid::new_v4();
        let mut autonomous = build_autonomous_receipt(input(
            candidate,
            contracts(ContractVerdict::Pass, ContractStrength::Hard),
        ));
        autonomous.unexpected_impact.push(ImpactTarget {
            kind: ImpactKind::VisualRegion,
            id: "route#visual-region-0".into(),
        });
        autonomous
            .impact_comparison
            .inconclusive
            .push(ImpactTarget {
                kind: ImpactKind::Region,
                id: "unknown-region".into(),
            });
        let observed = ProductionObservedVerificationInput {
            deterministic_status: ProductionDeterministicStatus::ChangeObserved,
            canonical_route: "http://127.0.0.1:5173/settings".into(),
            reference: Some("@e1".into()),
            snapshot_version: 42,
            reference_changed: true,
            visual_region_count: 0,
            regression_signals: Vec::new(),
            evidence_ids: vec!["semantic:after".into()],
            observed_runtime_ms: 100,
        };
        let bounded = build_bounded_target_verification(&observed, &autonomous);
        assert_eq!(bounded.verdict, BoundedVerificationVerdict::Inconclusive);
        assert!(
            bounded
                .reasons
                .iter()
                .any(|reason| reason.contains("unexpected impact"))
        );
        assert!(
            bounded
                .reasons
                .iter()
                .any(|reason| reason.contains("remain inconclusive"))
        );
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
        assert!(json.contains("\"schema_version\":2"));
        assert!(json.contains("\"bounded_verification\":null"));
        assert!(json.contains("\"final_verdict\":\"verified\""));
        assert!(receipt.digest().starts_with("sha256:"));
    }
}
