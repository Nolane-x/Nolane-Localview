use std::collections::BTreeSet;

use localview_mutation::{MutationOutcome, MutationVerdict};
use serde::{Deserialize, Serialize};

use crate::{LabError, LabFailureFlag, LabMetricKind, LabObservation, ResultEvidence};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationNotMeasuredReason {
    InvalidMutation,
    SkippedUnsafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "measurement_status")]
pub enum MutationLabRecord {
    Measured {
        observation: Box<LabObservation>,
        result_evidence: ResultEvidence,
    },
    NotMeasured {
        mutation_id: String,
        reason: MutationNotMeasuredReason,
    },
}

pub fn adapt_mutation_outcome(
    outcome: &MutationOutcome,
    seed_id: Option<&str>,
    comparison_profile_revision: &str,
    logical_sequence: u64,
) -> Result<MutationLabRecord, LabError> {
    if comparison_profile_revision.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision",
        });
    }

    let mutation_id = outcome.mutation_id.to_string();
    match outcome.verdict {
        MutationVerdict::Invalid => Ok(MutationLabRecord::NotMeasured {
            mutation_id,
            reason: MutationNotMeasuredReason::InvalidMutation,
        }),
        MutationVerdict::SkippedUnsafe => Ok(MutationLabRecord::NotMeasured {
            mutation_id,
            reason: MutationNotMeasuredReason::SkippedUnsafe,
        }),
        MutationVerdict::Killed | MutationVerdict::Survived => {
            let survived = outcome.verdict == MutationVerdict::Survived;
            let observation = LabObservation {
                observation_id: format!("mutation:{mutation_id}"),
                seed_id: seed_id.map(str::to_owned),
                expected_outcome: "mutant_killed".into(),
                observed_outcome: if survived {
                    "mutant_survived".into()
                } else {
                    "mutant_killed".into()
                },
                principal_expected: None,
                principal_dispatched: None,
                eligible_metrics: BTreeSet::from([LabMetricKind::Msr]),
                failure_flags: if survived {
                    BTreeSet::from([LabFailureFlag::MutationSurvived])
                } else {
                    BTreeSet::new()
                },
                evidence_refs: outcome.evidence_ids.iter().cloned().collect(),
                provider_backed: false,
                comparison_profile_revision: comparison_profile_revision.to_owned(),
                logical_sequence,
            };

            Ok(MutationLabRecord::Measured {
                observation: Box::new(observation),
                result_evidence: if survived {
                    ResultEvidence::MutantSurvived
                } else {
                    ResultEvidence::MutantKilled
                },
            })
        }
    }
}
