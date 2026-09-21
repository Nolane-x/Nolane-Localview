use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    CompiledStateSpace, Constraint, ProductState, StateDimension, StateSpacePlan, compile,
};

pub const MAX_AFFECTED_ROUTES: usize = 64;
pub const MAX_AFFECTED_REGIONS: usize = 128;
pub const MAX_AFFECTED_REFS: usize = 512;
pub const MAX_AFFECTED_CONTRACTS: usize = 256;
pub const MAX_AFFECTED_EVIDENCE_IDS: usize = 1024;
pub const MAX_AFFECTED_STATES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AffectedChangeIdentity {
    pub candidate_id: String,
    pub patch_digest: String,
    pub changed_project_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectedStateInput {
    pub base_revision: String,
    pub change: AffectedChangeIdentity,
    pub impacted_routes: BTreeSet<String>,
    pub impacted_regions: BTreeSet<String>,
    pub impacted_refs: BTreeSet<String>,
    pub impacted_contracts: BTreeSet<String>,
    pub dimensions: Vec<StateDimension>,
    pub constraints: Vec<Constraint>,
    pub max_states: usize,
    pub evidence_ids: Vec<String>,
    pub dependency_graph_complete: bool,
    pub denominator_known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectedStatePlan {
    pub base_revision: String,
    pub change: AffectedChangeIdentity,
    pub impacted_routes: Vec<String>,
    pub impacted_regions: Vec<String>,
    pub impacted_refs: Vec<String>,
    pub impacted_contracts: Vec<String>,
    pub state_dimensions: Vec<StateDimension>,
    pub compiled_states: Vec<ProductState>,
    pub risk_score: f32,
    pub evidence_provenance: Vec<String>,
    pub denominator_known: bool,
    pub eligible_state_count: Option<usize>,
    pub pair_coverage: Option<f32>,
    pub truncated: bool,
    pub incomplete: bool,
    pub incomplete_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AffectedStateCompileError {
    MissingBaseRevision,
    MissingCandidateIdentity,
    MissingPatchDigest,
    NoChangedProjectFiles,
    TooManyRoutes,
    TooManyRegions,
    TooManyRefs,
    TooManyContracts,
    TooManyEvidenceIds,
    InvalidStateBound,
    EmptyDimension { dimension: String },
    DuplicateDimension { dimension: String },
    DuplicateStateValue { dimension: String, value: String },
}

pub fn compile_affected_state_plan(
    input: &AffectedStateInput,
) -> Result<AffectedStatePlan, AffectedStateCompileError> {
    validate_input(input)?;

    let max_states = input.max_states.min(MAX_AFFECTED_STATES);
    let compiled = compile(&StateSpacePlan {
        dimensions: input.dimensions.clone(),
        constraints: input.constraints.clone(),
        max_states,
    });
    Ok(build_plan(input, compiled))
}

fn validate_input(input: &AffectedStateInput) -> Result<(), AffectedStateCompileError> {
    if input.base_revision.trim().is_empty() {
        return Err(AffectedStateCompileError::MissingBaseRevision);
    }
    if input.change.candidate_id.trim().is_empty() {
        return Err(AffectedStateCompileError::MissingCandidateIdentity);
    }
    if input.change.patch_digest.trim().is_empty() {
        return Err(AffectedStateCompileError::MissingPatchDigest);
    }
    if input.change.changed_project_files.is_empty() {
        return Err(AffectedStateCompileError::NoChangedProjectFiles);
    }
    if input.impacted_routes.len() > MAX_AFFECTED_ROUTES {
        return Err(AffectedStateCompileError::TooManyRoutes);
    }
    if input.impacted_regions.len() > MAX_AFFECTED_REGIONS {
        return Err(AffectedStateCompileError::TooManyRegions);
    }
    if input.impacted_refs.len() > MAX_AFFECTED_REFS {
        return Err(AffectedStateCompileError::TooManyRefs);
    }
    if input.impacted_contracts.len() > MAX_AFFECTED_CONTRACTS {
        return Err(AffectedStateCompileError::TooManyContracts);
    }
    if input.evidence_ids.len() > MAX_AFFECTED_EVIDENCE_IDS {
        return Err(AffectedStateCompileError::TooManyEvidenceIds);
    }
    if input.max_states == 0 || input.max_states > MAX_AFFECTED_STATES {
        return Err(AffectedStateCompileError::InvalidStateBound);
    }

    let mut dimensions = BTreeSet::new();
    for dimension in &input.dimensions {
        if dimension.values.is_empty() {
            return Err(AffectedStateCompileError::EmptyDimension {
                dimension: dimension.id.clone(),
            });
        }
        if !dimensions.insert(dimension.id.clone()) {
            return Err(AffectedStateCompileError::DuplicateDimension {
                dimension: dimension.id.clone(),
            });
        }
        let mut values = BTreeSet::new();
        for value in &dimension.values {
            if !values.insert(value.id.clone()) {
                return Err(AffectedStateCompileError::DuplicateStateValue {
                    dimension: dimension.id.clone(),
                    value: value.id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn build_plan(input: &AffectedStateInput, compiled: CompiledStateSpace) -> AffectedStatePlan {
    let eligible = compiled
        .total_unconstrained_combinations
        .saturating_sub(compiled.eliminated_by_constraints);
    let truncated = compiled.states.len() < eligible;

    let mut incomplete_reasons = Vec::new();
    if !input.dependency_graph_complete {
        incomplete_reasons.push("dependency/impact evidence is incomplete".into());
    }
    if !input.denominator_known {
        incomplete_reasons.push("affected-state denominator is unknown".into());
    }
    if truncated {
        incomplete_reasons.push(format!(
            "state compiler retained {} of {} eligible states",
            compiled.states.len(),
            eligible
        ));
    }
    if input.evidence_ids.is_empty() {
        incomplete_reasons.push("affected-state plan has no evidence provenance".into());
    }

    let frontier_size = input.impacted_routes.len()
        + input.impacted_regions.len()
        + input.impacted_refs.len()
        + input.impacted_contracts.len();
    let max_state_risk = compiled
        .states
        .iter()
        .map(|state| state.score)
        .fold(0.0f32, f32::max);
    let uncertainty_penalty = if incomplete_reasons.is_empty() {
        0.0
    } else {
        25.0
    };
    let risk_score =
        (frontier_size.min(100) as f32 + max_state_risk + uncertainty_penalty).clamp(0.0, 100.0);

    AffectedStatePlan {
        base_revision: input.base_revision.clone(),
        change: input.change.clone(),
        impacted_routes: bounded_sorted(&input.impacted_routes),
        impacted_regions: bounded_sorted(&input.impacted_regions),
        impacted_refs: bounded_sorted(&input.impacted_refs),
        impacted_contracts: bounded_sorted(&input.impacted_contracts),
        state_dimensions: input.dimensions.clone(),
        compiled_states: compiled.states,
        risk_score,
        evidence_provenance: dedupe_bounded(&input.evidence_ids, MAX_AFFECTED_EVIDENCE_IDS),
        denominator_known: input.denominator_known,
        eligible_state_count: input.denominator_known.then_some(eligible),
        pair_coverage: input.denominator_known.then_some(compiled.pair_coverage),
        truncated,
        incomplete: !incomplete_reasons.is_empty(),
        incomplete_reasons,
    }
}

fn bounded_sorted(values: &BTreeSet<String>) -> Vec<String> {
    values.iter().cloned().collect()
}

fn dedupe_bounded(values: &[String], limit: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            output.push(value.clone());
            if output.len() == limit {
                break;
            }
        }
    }
    output
}

pub fn state_values_by_dimension(plan: &AffectedStatePlan) -> BTreeMap<String, BTreeSet<String>> {
    let mut values = BTreeMap::<String, BTreeSet<String>>::new();
    for state in &plan.compiled_states {
        for (dimension, value) in &state.values {
            values
                .entry(dimension.clone())
                .or_default()
                .insert(value.clone());
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StateValue;

    fn dimension(id: &str, values: &[&str]) -> StateDimension {
        StateDimension {
            id: id.into(),
            values: values
                .iter()
                .map(|value| StateValue {
                    id: (*value).into(),
                    label: (*value).into(),
                    metadata: BTreeMap::new(),
                })
                .collect(),
            risk_weight: 1.0,
            boundary_values: BTreeSet::new(),
        }
    }

    fn input() -> AffectedStateInput {
        AffectedStateInput {
            base_revision: "0123456789012345678901234567890123456789".into(),
            change: AffectedChangeIdentity {
                candidate_id: "candidate-1".into(),
                patch_digest: "sha256:abc".into(),
                changed_project_files: vec!["src/App.tsx".into()],
            },
            impacted_routes: BTreeSet::from(["/".into()]),
            impacted_regions: BTreeSet::from(["hero".into()]),
            impacted_refs: BTreeSet::from(["@e1".into()]),
            impacted_contracts: BTreeSet::from(["layout.hero".into()]),
            dimensions: vec![
                dimension("viewport", &["mobile", "desktop"]),
                dimension("interaction", &["idle", "focused"]),
            ],
            constraints: vec![],
            max_states: 4,
            evidence_ids: vec!["ev-1".into()],
            dependency_graph_complete: true,
            denominator_known: true,
        }
    }

    #[test]
    fn compiles_bounded_affected_state_plan_with_known_denominator() {
        let plan = compile_affected_state_plan(&input()).unwrap();
        assert_eq!(plan.eligible_state_count, Some(4));
        assert_eq!(plan.compiled_states.len(), 4);
        assert!(!plan.truncated);
        assert!(!plan.incomplete);
        assert!(plan.pair_coverage.is_some());
    }

    #[test]
    fn unknown_denominator_never_claims_complete_coverage() {
        let mut input = input();
        input.denominator_known = false;
        let plan = compile_affected_state_plan(&input).unwrap();
        assert!(plan.incomplete);
        assert_eq!(plan.eligible_state_count, None);
        assert_eq!(plan.pair_coverage, None);
    }

    #[test]
    fn truncation_is_explicit_and_incomplete() {
        let mut input = input();
        input.max_states = 1;
        let plan = compile_affected_state_plan(&input).unwrap();
        assert!(plan.truncated);
        assert!(plan.incomplete);
        assert_eq!(plan.compiled_states.len(), 1);
    }

    #[test]
    fn incomplete_dependency_graph_escalates_plan_uncertainty() {
        let mut input = input();
        input.dependency_graph_complete = false;
        let plan = compile_affected_state_plan(&input).unwrap();
        assert!(plan.incomplete);
        assert!(plan.risk_score >= 25.0);
    }
}
