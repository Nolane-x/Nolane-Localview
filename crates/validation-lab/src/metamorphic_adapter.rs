use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{LabError, LabObservation, ResultEvidence};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetamorphicRelation {
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetamorphicCaseInput<'a, I> {
    pub case_id: &'a str,
    pub property_name: &'a str,
    pub base_outcome: &'a str,
    pub transformed_outcome: &'a str,
    pub relation: MetamorphicRelation,
    pub evidence_refs: I,
    pub comparison_profile_revision: &'a str,
    pub logical_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetamorphicLabRecord {
    pub observation: LabObservation,
    pub relation: MetamorphicRelation,
    pub relation_satisfied: bool,
    pub result_evidence: ResultEvidence,
}

pub fn adapt_metamorphic_case<I, S>(
    input: MetamorphicCaseInput<'_, I>,
) -> Result<MetamorphicLabRecord, LabError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let MetamorphicCaseInput {
        case_id,
        property_name,
        base_outcome,
        transformed_outcome,
        relation,
        evidence_refs,
        comparison_profile_revision,
        logical_sequence,
    } = input;

    if case_id.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField { field: "case_id" });
    }
    if property_name.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "property_name",
        });
    }
    if comparison_profile_revision.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision",
        });
    }

    let outcomes_equal = base_outcome == transformed_outcome;
    let relation_satisfied = match relation {
        MetamorphicRelation::Equal => outcomes_equal,
        MetamorphicRelation::NotEqual => !outcomes_equal,
    };
    let expected_outcome = match relation {
        MetamorphicRelation::Equal => "equal",
        MetamorphicRelation::NotEqual => "not_equal",
    };
    let observed_outcome = if outcomes_equal { "equal" } else { "not_equal" };

    let observation = LabObservation {
        observation_id: format!("metamorphic:{property_name}:{case_id}"),
        seed_id: None,
        expected_outcome: expected_outcome.to_owned(),
        observed_outcome: observed_outcome.to_owned(),
        principal_expected: None,
        principal_dispatched: None,
        eligible_metrics: BTreeSet::new(),
        failure_flags: BTreeSet::new(),
        evidence_refs: evidence_refs.into_iter().map(Into::into).collect(),
        provider_backed: false,
        comparison_profile_revision: comparison_profile_revision.to_owned(),
        logical_sequence,
    };

    Ok(MetamorphicLabRecord {
        observation,
        relation,
        relation_satisfied,
        result_evidence: if relation_satisfied {
            ResultEvidence::PreregisteredSeedPass
        } else {
            ResultEvidence::CounterexampleFound
        },
    })
}
