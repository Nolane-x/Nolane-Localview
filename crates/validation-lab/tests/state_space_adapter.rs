use std::collections::BTreeMap;

use localview_state_space::{StateDimension, StateSpacePlan, StateValue, compile};
use localview_validation_lab::{LabError, adapt_bounded_state_space};

fn dimension(id: &str, values: &[&str], boundaries: &[&str]) -> StateDimension {
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
        boundary_values: boundaries.iter().map(|value| (*value).into()).collect(),
    }
}

fn plan(max_states: usize) -> StateSpacePlan {
    StateSpacePlan {
        dimensions: vec![
            dimension("theme", &["light", "dark"], &["dark"]),
            dimension("viewport", &["mobile", "desktop"], &["mobile"]),
        ],
        constraints: Vec::new(),
        max_states,
    }
}

#[test]
fn l4_adapter_reuses_state_space_compiler_and_preserves_exact_bound_provenance() {
    let plan = plan(3);
    let expected = compile(&plan);

    let adapted = adapt_bounded_state_space(&plan, 3).expect("adapt bounded state space");

    assert_eq!(adapted.declared_bound, 3);
    assert_eq!(adapted.executed_state_count, expected.states.len() as u64);
    assert_eq!(
        adapted.state_keys,
        expected
            .states
            .iter()
            .map(|state| state.key())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        adapted.total_unconstrained_combinations,
        expected.total_unconstrained_combinations as u64
    );
    assert_eq!(
        adapted.eliminated_by_constraints,
        expected.eliminated_by_constraints as u64
    );
}

#[test]
fn l4_adapter_rejects_declared_bound_drift_from_compiler_plan() {
    let error = adapt_bounded_state_space(&plan(2), 3).expect_err("bound drift must fail closed");

    assert_eq!(
        error,
        LabError::StateSpaceBoundMismatch {
            declared_bound: 3,
            plan_max_states: 2,
        }
    );
}

#[test]
fn l4_adapter_rejects_zero_bound_instead_of_inheriting_compiler_minimum_one() {
    let error = adapt_bounded_state_space(&plan(0), 0).expect_err("zero bound must fail closed");

    assert_eq!(
        error,
        LabError::InvalidStateSpaceBound { declared_bound: 0 }
    );
}
