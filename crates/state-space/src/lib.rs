#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateValue {
    pub id: String,
    pub label: String,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateDimension {
    pub id: String,
    pub values: Vec<StateValue>,
    pub risk_weight: f32,
    pub boundary_values: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    ForbidPair { left_dimension: String, left_value: String, right_dimension: String, right_value: String },
    RequirePair { when_dimension: String, when_value: String, required_dimension: String, allowed_values: BTreeSet<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProductState {
    pub values: BTreeMap<String, String>,
    pub score: f32,
    pub provenance: Vec<String>,
}

impl ProductState {
    pub fn key(&self) -> String {
        self.values.iter().map(|(dimension, value)| format!("{dimension}={value}")).collect::<Vec<_>>().join("|")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateSpacePlan {
    pub dimensions: Vec<StateDimension>,
    pub constraints: Vec<Constraint>,
    pub max_states: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledStateSpace {
    pub states: Vec<ProductState>,
    pub total_unconstrained_combinations: usize,
    pub pair_coverage: f32,
    pub eliminated_by_constraints: usize,
}

pub fn compile(plan: &StateSpacePlan) -> CompiledStateSpace {
    if plan.dimensions.is_empty() {
        return CompiledStateSpace { states: Vec::new(), total_unconstrained_combinations: 0, pair_coverage: 1.0, eliminated_by_constraints: 0 };
    }
    let total_unconstrained_combinations = plan.dimensions.iter().map(|dimension| dimension.values.len().max(1)).product();
    let mut all = Vec::new();
    expand_states(&plan.dimensions, 0, &mut BTreeMap::new(), &mut all);
    let before_constraints = all.len();
    all.retain(|state| satisfies_constraints(&state.values, &plan.constraints));
    let eliminated_by_constraints = before_constraints.saturating_sub(all.len());

    for state in &mut all {
        state.score = risk_score(state, &plan.dimensions);
        state.provenance = state.values.iter().map(|(dimension, value)| format!("dimension:{dimension}={value}")).collect();
    }
    all.sort_by(|left, right| right.score.total_cmp(&left.score).then_with(|| left.key().cmp(&right.key())));

    let target = plan.max_states.max(1).min(all.len());
    let universe = pair_universe(&all, &plan.dimensions);
    let mut covered = HashSet::new();
    let mut selected = Vec::new();
    let mut remaining = all;

    while selected.len() < target && !remaining.is_empty() {
        let (best_index, _) = remaining.iter().enumerate().map(|(index, state)| {
            let pairs = state_pairs(state, &plan.dimensions);
            let new_pairs = pairs.iter().filter(|pair| !covered.contains(*pair)).count();
            let boundary_bonus = boundary_hits(state, &plan.dimensions);
            let utility = new_pairs as f32 * 10.0 + boundary_bonus as f32 * 3.0 + state.score;
            (index, utility)
        }).max_by(|left, right| left.1.total_cmp(&right.1)).expect("remaining is not empty");
        let picked = remaining.remove(best_index);
        covered.extend(state_pairs(&picked, &plan.dimensions));
        selected.push(picked);
    }

    let pair_coverage = if universe.is_empty() { 1.0 } else { covered.intersection(&universe).count() as f32 / universe.len() as f32 };
    CompiledStateSpace { states: selected, total_unconstrained_combinations, pair_coverage, eliminated_by_constraints }
}

fn expand_states(dimensions: &[StateDimension], index: usize, current: &mut BTreeMap<String, String>, output: &mut Vec<ProductState>) {
    if index == dimensions.len() {
        output.push(ProductState { values: current.clone(), score: 0.0, provenance: Vec::new() });
        return;
    }
    let dimension = &dimensions[index];
    for value in &dimension.values {
        current.insert(dimension.id.clone(), value.id.clone());
        expand_states(dimensions, index + 1, current, output);
    }
    current.remove(&dimension.id);
}

fn satisfies_constraints(values: &BTreeMap<String, String>, constraints: &[Constraint]) -> bool {
    constraints.iter().all(|constraint| match constraint {
        Constraint::ForbidPair { left_dimension, left_value, right_dimension, right_value } => {
            !(values.get(left_dimension) == Some(left_value) && values.get(right_dimension) == Some(right_value))
        }
        Constraint::RequirePair { when_dimension, when_value, required_dimension, allowed_values } => {
            values.get(when_dimension) != Some(when_value) || values.get(required_dimension).is_some_and(|value| allowed_values.contains(value))
        }
    })
}

fn risk_score(state: &ProductState, dimensions: &[StateDimension]) -> f32 {
    dimensions.iter().map(|dimension| {
        let boundary = state.values.get(&dimension.id).is_some_and(|value| dimension.boundary_values.contains(value));
        dimension.risk_weight.max(0.0) * if boundary { 1.5 } else { 1.0 }
    }).sum()
}

fn pair_universe(states: &[ProductState], dimensions: &[StateDimension]) -> HashSet<String> {
    states.iter().flat_map(|state| state_pairs(state, dimensions)).collect()
}

fn state_pairs(state: &ProductState, dimensions: &[StateDimension]) -> Vec<String> {
    let mut result = Vec::new();
    for left in 0..dimensions.len() {
        for right in (left + 1)..dimensions.len() {
            let left_dimension = &dimensions[left].id;
            let right_dimension = &dimensions[right].id;
            if let (Some(left_value), Some(right_value)) = (state.values.get(left_dimension), state.values.get(right_dimension)) {
                result.push(format!("{left_dimension}={left_value}|{right_dimension}={right_value}"));
            }
        }
    }
    result
}

fn boundary_hits(state: &ProductState, dimensions: &[StateDimension]) -> usize {
    dimensions.iter().filter(|dimension| state.values.get(&dimension.id).is_some_and(|value| dimension.boundary_values.contains(value))).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dimension(id: &str, values: &[&str]) -> StateDimension {
        StateDimension {
            id: id.into(),
            values: values.iter().map(|value| StateValue { id: (*value).into(), label: (*value).into(), metadata: BTreeMap::new() }).collect(),
            risk_weight: 1.0,
            boundary_values: BTreeSet::new(),
        }
    }

    #[test]
    fn compiler_respects_forbidden_pairs_and_reduces_state_set() {
        let plan = StateSpacePlan {
            dimensions: vec![dimension("theme", &["light", "dark"]), dimension("locale", &["en", "ar"]), dimension("viewport", &["mobile", "desktop"])],
            constraints: vec![Constraint::ForbidPair { left_dimension: "theme".into(), left_value: "dark".into(), right_dimension: "locale".into(), right_value: "ar".into() }],
            max_states: 4,
        };
        let compiled = compile(&plan);
        assert_eq!(compiled.total_unconstrained_combinations, 8);
        assert!(compiled.states.len() <= 4);
        assert!(compiled.states.iter().all(|state| !(state.values["theme"] == "dark" && state.values["locale"] == "ar")));
        assert!(compiled.pair_coverage > 0.5);
    }
}