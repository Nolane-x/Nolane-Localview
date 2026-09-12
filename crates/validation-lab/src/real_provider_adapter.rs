use std::collections::BTreeSet;

use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness,
};
use serde::{Deserialize, Serialize};

use crate::{
    CanonicalDigest, LabError, LabFailureFlag, LabMetricKind, LabObservation, ResultEvidence,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum RealProviderObservedOutcome {
    Asserted(String),
    Unknown,
    Inconclusive,
    Unsupported,
    ConservativeBlock,
}

impl RealProviderObservedOutcome {
    fn asserted_value(&self) -> Option<&str> {
        match self {
            Self::Asserted(value) => Some(value.as_str()),
            Self::Unknown
            | Self::Inconclusive
            | Self::Unsupported
            | Self::ConservativeBlock => None,
        }
    }

    fn display_value(&self) -> String {
        match self {
            Self::Asserted(value) => value.clone(),
            Self::Unknown => "unknown".into(),
            Self::Inconclusive => "inconclusive".into(),
            Self::Unsupported => "unsupported".into(),
            Self::ConservativeBlock => "conservative_block".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealProviderGroundTruth {
    pub canonical_outcome: String,
    pub digest: CanonicalDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "case")]
pub enum RealProviderCaseKind {
    W01MissingPropertyEvent {
        continuity: EventContinuityState,
        reconciliation: Option<ReconciliationCompleteness>,
        accepted_as_fresh: bool,
        accepted_as_reconciled: bool,
    },
    W02RecreatedElement {
        previous_provider_incarnation: ProviderIncarnationRef,
        current_provider_incarnation: ProviderIncarnationRef,
        opaque_provider_element_id: String,
        provider_identity_reuse_observed: bool,
        accepted_previous_identity_as_current: bool,
    },
    W03VirtualizedItemRealization {
        placeholder_blocked_before_realization: bool,
        fresh_cut_after_realization: bool,
        realized_current_after_fresh_cut: bool,
    },
    W04UnsupportedInvoke {
        invoke_support_unsupported: bool,
        dispatch_attempted: bool,
        side_effect_observed: bool,
    },
    W05ProviderHang {
        caller_returned_bounded: bool,
        poisoned_worker_reused: bool,
        provider_reacquired: bool,
        stale_authority_survived_reacquire: bool,
    },
    W06ProviderReacquire {
        previous_provider_incarnation: ProviderIncarnationRef,
        current_provider_incarnation: ProviderIncarnationRef,
        stale_authority_survived_reacquire: bool,
        cleanup_to_baseline: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealProviderCaseInput<'a> {
    pub case_id: &'a str,
    pub seed_app_digest: &'a str,
    pub platform_profile_revision: &'a str,
    pub environment_artifact_digest: &'a str,
    pub provider_evidence_refs: BTreeSet<String>,
    pub ground_truth: RealProviderGroundTruth,
    pub observed_outcome: RealProviderObservedOutcome,
    pub case_kind: RealProviderCaseKind,
    pub comparison_profile_revision: &'a str,
    pub logical_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealProviderLabRecord {
    pub observation: LabObservation,
    pub result_evidence: Option<ResultEvidence>,
}

pub fn adapt_real_provider_case(
    input: RealProviderCaseInput<'_>,
) -> Result<RealProviderLabRecord, LabError> {
    validate_authority_field("case_id", input.case_id)?;
    validate_authority_field("seed_app_digest", input.seed_app_digest)?;
    validate_authority_field(
        "platform_profile_revision",
        input.platform_profile_revision,
    )?;
    validate_authority_field(
        "environment_artifact_digest",
        input.environment_artifact_digest,
    )?;
    validate_authority_field("ground_truth_digest", &input.ground_truth.digest.0)?;
    validate_authority_field(
        "comparison_profile_revision",
        input.comparison_profile_revision,
    )?;

    let mut eligible_metrics = BTreeSet::new();
    let mut failure_flags = BTreeSet::new();

    let semantic_counterexample = apply_case_semantics(
        &input.case_kind,
        &mut eligible_metrics,
        &mut failure_flags,
    )?;

    if let Some(asserted) = input.observed_outcome.asserted_value() {
        eligible_metrics.insert(LabMetricKind::Rpomr);
        if asserted != input.ground_truth.canonical_outcome {
            failure_flags.insert(LabFailureFlag::RealProviderOracleMismatch);
        }
    }

    let result_evidence = if semantic_counterexample || !failure_flags.is_empty() {
        Some(ResultEvidence::CounterexampleFound)
    } else if input.observed_outcome.asserted_value().is_some() {
        Some(ResultEvidence::RealProviderIntegrationPass)
    } else {
        None
    };

    let mut evidence_refs = input.provider_evidence_refs;
    evidence_refs.insert(format!("ground-truth:{}", input.ground_truth.digest.0));
    evidence_refs.insert(format!(
        "environment:{}",
        input.environment_artifact_digest
    ));
    evidence_refs.insert(format!("seed-app:{}", input.seed_app_digest));
    evidence_refs.insert(format!("platform:{}", input.platform_profile_revision));

    Ok(RealProviderLabRecord {
        observation: LabObservation {
            observation_id: format!("real_provider:{}", input.case_id),
            seed_id: Some(input.case_id.to_owned()),
            expected_outcome: input.ground_truth.canonical_outcome,
            observed_outcome: input.observed_outcome.display_value(),
            principal_expected: None,
            principal_dispatched: None,
            eligible_metrics,
            failure_flags,
            evidence_refs,
            provider_backed: true,
            comparison_profile_revision: input.comparison_profile_revision.to_owned(),
            logical_sequence: input.logical_sequence,
        },
        result_evidence,
    })
}

pub fn derive_real_provider_campaign_evidence(
    records: &[RealProviderLabRecord],
) -> Option<ResultEvidence> {
    if records.is_empty() {
        return None;
    }
    if records
        .iter()
        .any(|record| record.result_evidence == Some(ResultEvidence::CounterexampleFound))
    {
        return Some(ResultEvidence::CounterexampleFound);
    }
    if records.iter().any(|record| record.result_evidence.is_none()) {
        return None;
    }
    if records.iter().all(|record| {
        record.result_evidence == Some(ResultEvidence::RealProviderIntegrationPass)
    }) {
        return Some(ResultEvidence::RealProviderIntegrationPass);
    }
    None
}

fn apply_case_semantics(
    case_kind: &RealProviderCaseKind,
    eligible_metrics: &mut BTreeSet<LabMetricKind>,
    failure_flags: &mut BTreeSet<LabFailureFlag>,
) -> Result<bool, LabError> {
    let semantic_counterexample = match case_kind {
        RealProviderCaseKind::W01MissingPropertyEvent {
            continuity,
            reconciliation,
            accepted_as_fresh,
            accepted_as_reconciled,
        } => {
            eligible_metrics.insert(LabMetricKind::Eoffr);
            eligible_metrics.insert(LabMetricKind::Rmr);

            let reconciliation_established = matches!(
                reconciliation,
                Some(ReconciliationCompleteness::Established)
            );
            if *accepted_as_fresh
                && *continuity != EventContinuityState::Continuous
                && !reconciliation_established
            {
                failure_flags.insert(LabFailureFlag::EventOnlyFalseFreshness);
            }
            if *accepted_as_reconciled && !reconciliation_established {
                failure_flags.insert(LabFailureFlag::ReconciliationMiss);
            }
            false
        }
        RealProviderCaseKind::W02RecreatedElement {
            previous_provider_incarnation,
            current_provider_incarnation,
            opaque_provider_element_id,
            provider_identity_reuse_observed,
            accepted_previous_identity_as_current,
        } => {
            // W02 is an element-lifetime/ABA campaign. A control can be destroyed
            // and recreated while the same provider worker remains alive, so
            // provider reincarnation is neither required nor sufficient evidence
            // for the element identity boundary. W06 owns provider-reacquire
            // reincarnation semantics.
            validate_provider_incarnation(
                "previous_provider_incarnation",
                previous_provider_incarnation,
            )?;
            validate_provider_incarnation(
                "current_provider_incarnation",
                current_provider_incarnation,
            )?;
            validate_authority_field(
                "opaque_provider_element_id",
                opaque_provider_element_id,
            )?;

            if *provider_identity_reuse_observed {
                eligible_metrics.insert(LabMetricKind::Piaer);
                if *accepted_previous_identity_as_current {
                    failure_flags.insert(LabFailureFlag::ProviderIdAbaEscape);
                }
            }
            false
        }
        RealProviderCaseKind::W03VirtualizedItemRealization {
            placeholder_blocked_before_realization,
            fresh_cut_after_realization,
            realized_current_after_fresh_cut,
        } => {
            !*placeholder_blocked_before_realization
                || !*fresh_cut_after_realization
                || !*realized_current_after_fresh_cut
        }
        RealProviderCaseKind::W04UnsupportedInvoke {
            invoke_support_unsupported,
            dispatch_attempted,
            side_effect_observed,
        } => {
            !*invoke_support_unsupported || *dispatch_attempted || *side_effect_observed
        }
        RealProviderCaseKind::W05ProviderHang {
            caller_returned_bounded,
            poisoned_worker_reused,
            provider_reacquired,
            stale_authority_survived_reacquire,
        } => {
            if *stale_authority_survived_reacquire {
                eligible_metrics.insert(LabMetricKind::Scar);
                failure_flags.insert(LabFailureFlag::StaleCacheAuthority);
            }
            !*caller_returned_bounded
                || *poisoned_worker_reused
                || !*provider_reacquired
                || *stale_authority_survived_reacquire
        }
        RealProviderCaseKind::W06ProviderReacquire {
            previous_provider_incarnation,
            current_provider_incarnation,
            stale_authority_survived_reacquire,
            cleanup_to_baseline,
        } => {
            validate_provider_incarnation(
                "previous_provider_incarnation",
                previous_provider_incarnation,
            )?;
            validate_provider_incarnation(
                "current_provider_incarnation",
                current_provider_incarnation,
            )?;
            if previous_provider_incarnation == current_provider_incarnation {
                return Err(LabError::InvalidRealProviderScenario {
                    reason: "w06_requires_distinct_provider_incarnations",
                });
            }

            eligible_metrics.insert(LabMetricKind::Scar);
            eligible_metrics.insert(LabMetricKind::Cbfr);
            if *stale_authority_survived_reacquire {
                failure_flags.insert(LabFailureFlag::StaleCacheAuthority);
            }
            if !*cleanup_to_baseline {
                failure_flags.insert(LabFailureFlag::CleanupToBaselineFailure);
            }
            false
        }
    };
    Ok(semantic_counterexample)
}

fn validate_authority_field(field: &'static str, value: &str) -> Result<(), LabError> {
    if value.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField { field });
    }
    Ok(())
}

fn validate_provider_incarnation(
    field: &'static str,
    incarnation: &ProviderIncarnationRef,
) -> Result<(), LabError> {
    validate_authority_field(field, incarnation.as_str())
}
