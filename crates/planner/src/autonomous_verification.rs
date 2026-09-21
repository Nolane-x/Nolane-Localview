use std::collections::BTreeSet;

use localview_state_space::{AffectedStatePlan, state_values_by_dimension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RevalidationMode {
    Partial,
    Escalated,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevalidationUniverse {
    pub routes: BTreeSet<String>,
    pub regions: BTreeSet<String>,
    pub refs: BTreeSet<String>,
    pub contracts: BTreeSet<String>,
    pub responsive_widths: BTreeSet<String>,
    pub flow_checkpoints: BTreeSet<String>,
    pub visual_baselines: BTreeSet<String>,
    pub source_semantic_checks: BTreeSet<String>,
    pub state_keys: BTreeSet<String>,
    pub denominator_known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PartialRevalidationInput {
    pub affected: AffectedStatePlan,
    pub impacted_flow_checkpoints: BTreeSet<String>,
    pub relevant_visual_baselines: BTreeSet<String>,
    pub source_semantic_checks: BTreeSet<String>,
    pub known_universe: Option<RevalidationUniverse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartialRevalidationPlan {
    pub mode: RevalidationMode,
    pub routes: Vec<String>,
    pub regions: Vec<String>,
    pub refs: Vec<String>,
    pub contracts: Vec<String>,
    pub responsive_widths: Vec<String>,
    pub flow_checkpoints: Vec<String>,
    pub visual_baselines: Vec<String>,
    pub source_semantic_checks: Vec<String>,
    pub state_keys: Vec<String>,
    pub escalation_reasons: Vec<String>,
    pub denominator_known: bool,
    pub complete_claim_allowed: bool,
}

pub fn plan_partial_revalidation(input: &PartialRevalidationInput) -> PartialRevalidationPlan {
    let state_values = state_values_by_dimension(&input.affected);
    let mut widths = BTreeSet::new();
    for (dimension, values) in state_values {
        let normalized = dimension.to_ascii_lowercase();
        if normalized.contains("viewport")
            || normalized.contains("width")
            || normalized.contains("responsive")
        {
            widths.extend(values);
        }
    }

    let mut routes = input.affected.impacted_routes.iter().cloned().collect();
    let mut regions = input.affected.impacted_regions.iter().cloned().collect();
    let mut refs = input.affected.impacted_refs.iter().cloned().collect();
    let mut contracts = input.affected.impacted_contracts.iter().cloned().collect();
    let mut flow_checkpoints = input.impacted_flow_checkpoints.clone();
    let mut visual_baselines = input.relevant_visual_baselines.clone();
    let mut source_semantic_checks = input.source_semantic_checks.clone();
    let mut state_keys = input
        .affected
        .compiled_states
        .iter()
        .map(|state| state.key())
        .collect::<BTreeSet<_>>();

    let mut escalation_reasons = Vec::new();
    if input.affected.incomplete {
        escalation_reasons.extend(input.affected.incomplete_reasons.clone());
    }
    if input.affected.truncated {
        escalation_reasons.push("affected state-space was truncated".into());
    }
    if !input.affected.denominator_known {
        escalation_reasons.push("affected-state denominator is unknown".into());
    }

    let mode = if escalation_reasons.is_empty() {
        RevalidationMode::Partial
    } else {
        RevalidationMode::Escalated
    };

    let mut denominator_known = input.affected.denominator_known && !input.affected.truncated;
    if mode == RevalidationMode::Escalated {
        if let Some(universe) = &input.known_universe {
            routes.extend(universe.routes.iter().cloned());
            regions.extend(universe.regions.iter().cloned());
            refs.extend(universe.refs.iter().cloned());
            contracts.extend(universe.contracts.iter().cloned());
            widths.extend(universe.responsive_widths.iter().cloned());
            flow_checkpoints.extend(universe.flow_checkpoints.iter().cloned());
            visual_baselines.extend(universe.visual_baselines.iter().cloned());
            source_semantic_checks.extend(universe.source_semantic_checks.iter().cloned());
            state_keys.extend(universe.state_keys.iter().cloned());
            denominator_known = universe.denominator_known;
            if !universe.denominator_known {
                escalation_reasons
                    .push("escalation universe denominator is also unknown".into());
            }
        } else {
            denominator_known = false;
            escalation_reasons.push(
                "no complete revalidation universe is available for required escalation".into(),
            );
        }
    }

    escalation_reasons.sort();
    escalation_reasons.dedup();

    PartialRevalidationPlan {
        mode,
        routes: sorted(routes),
        regions: sorted(regions),
        refs: sorted(refs),
        contracts: sorted(contracts),
        responsive_widths: sorted(widths),
        flow_checkpoints: sorted(flow_checkpoints),
        visual_baselines: sorted(visual_baselines),
        source_semantic_checks: sorted(source_semantic_checks),
        state_keys: sorted(state_keys),
        escalation_reasons,
        denominator_known,
        complete_claim_allowed: denominator_known,
    }
}

fn sorted(values: BTreeSet<String>) -> Vec<String> {
    values.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use localview_state_space::{
        AffectedChangeIdentity, AffectedStateInput, StateDimension, StateValue,
        compile_affected_state_plan,
    };

    use super::*;

    fn affected(complete: bool, max_states: usize) -> AffectedStatePlan {
        compile_affected_state_plan(&AffectedStateInput {
            base_revision: "abc".into(),
            change: AffectedChangeIdentity {
                candidate_id: "candidate".into(),
                patch_digest: "sha256:x".into(),
                changed_project_files: vec!["src/App.tsx".into()],
            },
            impacted_routes: BTreeSet::from(["/account".into()]),
            impacted_regions: BTreeSet::from(["profile".into()]),
            impacted_refs: BTreeSet::from(["@e1".into()]),
            impacted_contracts: BTreeSet::from(["contract.profile".into()]),
            dimensions: vec![StateDimension {
                id: "viewport_width".into(),
                values: vec![
                    StateValue {
                        id: "375".into(),
                        label: "mobile".into(),
                        metadata: BTreeMap::new(),
                    },
                    StateValue {
                        id: "1440".into(),
                        label: "desktop".into(),
                        metadata: BTreeMap::new(),
                    },
                ],
                risk_weight: 1.0,
                boundary_values: BTreeSet::from(["375".into()]),
            }],
            constraints: vec![],
            max_states,
            evidence_ids: vec!["ev".into()],
            dependency_graph_complete: complete,
            denominator_known: complete,
        })
        .unwrap()
    }

    #[test]
    fn complete_small_change_uses_partial_revalidation() {
        let plan = plan_partial_revalidation(&PartialRevalidationInput {
            affected: affected(true, 2),
            impacted_flow_checkpoints: BTreeSet::from(["save-profile".into()]),
            relevant_visual_baselines: BTreeSet::from(["profile-card".into()]),
            source_semantic_checks: BTreeSet::from(["source-hash".into()]),
            known_universe: None,
        });
        assert_eq!(plan.mode, RevalidationMode::Partial);
        assert!(plan.complete_claim_allowed);
        assert_eq!(plan.responsive_widths, vec!["1440", "375"]);
    }

    #[test]
    fn incomplete_dependency_graph_requires_scope_escalation() {
        let plan = plan_partial_revalidation(&PartialRevalidationInput {
            affected: affected(false, 2),
            impacted_flow_checkpoints: BTreeSet::new(),
            relevant_visual_baselines: BTreeSet::new(),
            source_semantic_checks: BTreeSet::new(),
            known_universe: None,
        });
        assert_eq!(plan.mode, RevalidationMode::Escalated);
        assert!(!plan.complete_claim_allowed);
        assert!(plan.escalation_reasons.iter().any(|reason| {
            reason.contains("dependency/impact evidence is incomplete")
        }));
    }

    #[test]
    fn complete_known_universe_can_satisfy_required_escalation() {
        let universe = RevalidationUniverse {
            routes: BTreeSet::from(["/".into(), "/account".into()]),
            contracts: BTreeSet::from(["global".into()]),
            responsive_widths: BTreeSet::from(["768".into()]),
            state_keys: BTreeSet::from(["global=default".into()]),
            denominator_known: true,
            ..Default::default()
        };
        let plan = plan_partial_revalidation(&PartialRevalidationInput {
            affected: affected(false, 2),
            impacted_flow_checkpoints: BTreeSet::new(),
            relevant_visual_baselines: BTreeSet::new(),
            source_semantic_checks: BTreeSet::new(),
            known_universe: Some(universe),
        });
        assert_eq!(plan.mode, RevalidationMode::Escalated);
        assert!(plan.complete_claim_allowed);
        assert!(plan.routes.contains(&"/".into()));
    }

    #[test]
    fn truncated_state_space_escalates_instead_of_claiming_complete() {
        let plan = plan_partial_revalidation(&PartialRevalidationInput {
            affected: affected(true, 1),
            impacted_flow_checkpoints: BTreeSet::new(),
            relevant_visual_baselines: BTreeSet::new(),
            source_semantic_checks: BTreeSet::new(),
            known_universe: None,
        });
        assert_eq!(plan.mode, RevalidationMode::Escalated);
        assert!(!plan.complete_claim_allowed);
    }
}
