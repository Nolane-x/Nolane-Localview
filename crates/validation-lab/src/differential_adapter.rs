use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{LabError, LabFailureFlag, LabMetricKind, LabObservation, ResultEvidence};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialVector {
    pub vector_id: String,
    pub reference_outcome: String,
    pub candidate_outcome: String,
    pub evidence_refs: BTreeSet<String>,
    pub logical_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialVectorSetInput {
    pub vector_set_id: String,
    pub comparison_mode: String,
    pub comparison_profile_revision: String,
    pub vectors: Vec<DifferentialVector>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialVectorSetLabRecord {
    pub observations: Vec<LabObservation>,
    pub result_evidence: Option<ResultEvidence>,
    pub first_divergence_vector_id: Option<String>,
}

pub fn adapt_differential_vector_set(
    input: DifferentialVectorSetInput,
) -> Result<DifferentialVectorSetLabRecord, LabError> {
    if input.comparison_profile_revision.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision",
        });
    }
    if input.comparison_mode != "exact" {
        return Err(LabError::UnsupportedDifferentialComparisonMode {
            mode: input.comparison_mode,
        });
    }

    let mut vector_ids = BTreeSet::new();
    for vector in &input.vectors {
        if !vector_ids.insert(vector.vector_id.clone()) {
            return Err(LabError::DuplicateDifferentialVectorId {
                vector_id: vector.vector_id.clone(),
            });
        }
    }

    let mut first_divergence_vector_id = None;
    let mut observations = Vec::with_capacity(input.vectors.len());

    for vector in input.vectors {
        let diverged = vector.reference_outcome != vector.candidate_outcome;
        if diverged && first_divergence_vector_id.is_none() {
            first_divergence_vector_id = Some(vector.vector_id.clone());
        }

        observations.push(LabObservation {
            observation_id: format!("differential:{}:{}", input.vector_set_id, vector.vector_id),
            seed_id: None,
            expected_outcome: vector.reference_outcome,
            observed_outcome: vector.candidate_outcome,
            principal_expected: None,
            principal_dispatched: None,
            eligible_metrics: BTreeSet::from([LabMetricKind::Crdr]),
            failure_flags: if diverged {
                BTreeSet::from([LabFailureFlag::CrossReducerDivergence])
            } else {
                BTreeSet::new()
            },
            evidence_refs: vector.evidence_refs,
            provider_backed: false,
            comparison_profile_revision: input.comparison_profile_revision.clone(),
            logical_sequence: vector.logical_sequence,
        });
    }

    let result_evidence = if observations.is_empty() {
        None
    } else if first_divergence_vector_id.is_some() {
        Some(ResultEvidence::DifferentialDivergenceFound)
    } else {
        Some(ResultEvidence::DifferentialEquivalentWithinVectorSet)
    };

    Ok(DifferentialVectorSetLabRecord {
        observations,
        result_evidence,
        first_divergence_vector_id,
    })
}
