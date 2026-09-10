use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{LabError, LabObservation, LabSeed, ResultEvidence};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSeedLabRecord {
    pub observation: LabObservation,
    pub result_evidence: ResultEvidence,
    pub forbidden_outcome_observed: bool,
}

pub fn adapt_semantic_seed_outcome<I, S>(
    seed: &LabSeed,
    observed_outcome: &str,
    evidence_refs: I,
    comparison_profile_revision: &str,
    logical_sequence: u64,
) -> Result<SemanticSeedLabRecord, LabError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    if comparison_profile_revision.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision",
        });
    }

    if seed.comparison_mode != "exact" {
        return Err(LabError::UnsupportedSemanticComparisonMode {
            mode: seed.comparison_mode.clone(),
        });
    }

    let matched = observed_outcome == seed.expected_semantic_outcome;
    let forbidden_outcome_observed = seed.forbidden_outcomes.contains(observed_outcome);

    let observation = LabObservation {
        observation_id: format!("semantic-seed:{}", seed.identity.seed_id),
        seed_id: Some(seed.identity.seed_id.clone()),
        expected_outcome: seed.expected_semantic_outcome.clone(),
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

    Ok(SemanticSeedLabRecord {
        observation,
        result_evidence: if matched {
            ResultEvidence::PreregisteredSeedPass
        } else {
            ResultEvidence::CounterexampleFound
        },
        forbidden_outcome_observed,
    })
}
