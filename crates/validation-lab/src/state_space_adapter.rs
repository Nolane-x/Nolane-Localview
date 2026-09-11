use localview_state_space::{StateSpacePlan, compile};

use crate::LabError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedStateSpaceLabRecord {
    pub declared_bound: u64,
    pub executed_state_count: u64,
    pub state_keys: Vec<String>,
    pub total_unconstrained_combinations: u64,
    pub eliminated_by_constraints: u64,
}

pub fn adapt_bounded_state_space(
    plan: &StateSpacePlan,
    declared_bound: usize,
) -> Result<BoundedStateSpaceLabRecord, LabError> {
    if declared_bound == 0 {
        return Err(LabError::InvalidStateSpaceBound { declared_bound });
    }

    if plan.max_states != declared_bound {
        return Err(LabError::StateSpaceBoundMismatch {
            declared_bound,
            plan_max_states: plan.max_states,
        });
    }

    let compiled = compile(plan);

    Ok(BoundedStateSpaceLabRecord {
        declared_bound: declared_bound as u64,
        executed_state_count: compiled.states.len() as u64,
        state_keys: compiled.states.iter().map(|state| state.key()).collect(),
        total_unconstrained_combinations: compiled.total_unconstrained_combinations as u64,
        eliminated_by_constraints: compiled.eliminated_by_constraints as u64,
    })
}
