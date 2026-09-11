use std::collections::BTreeSet;

use localview_protocol::{
    EventContinuityState, PrincipalRef, ProviderIncarnationRef, ReconciliationCompleteness,
};
use serde::{Deserialize, Serialize};

use crate::{LabError, LabFailureFlag, LabMetricKind, LabObservation, ResultEvidence};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "scenario")]
pub enum FakeProviderScenario {
    Freshness {
        continuity: EventContinuityState,
        reconciliation: Option<ReconciliationCompleteness>,
        accepted_as_fresh: bool,
    },
    Reconciliation {
        completeness: ReconciliationCompleteness,
        accepted_as_reconciled: bool,
    },
    ProviderAba {
        previous_provider_incarnation: ProviderIncarnationRef,
        current_provider_incarnation: ProviderIncarnationRef,
        opaque_provider_element_id: String,
        accepted_previous_identity_as_current: bool,
    },
    PrincipalDispatch {
        expected_principal: PrincipalRef,
        dispatched_principal: PrincipalRef,
    },
    PrincipalVisibility {
        authorized_principal: PrincipalRef,
        exposed_principal: PrincipalRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeProviderCaseInput<'a> {
    pub case_id: &'a str,
    pub scenario: FakeProviderScenario,
    pub evidence_refs: BTreeSet<String>,
    pub comparison_profile_revision: &'a str,
    pub logical_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakeProviderLabRecord {
    pub observation: LabObservation,
    pub result_evidence: ResultEvidence,
}

pub fn adapt_fake_provider_case(
    input: FakeProviderCaseInput<'_>,
) -> Result<FakeProviderLabRecord, LabError> {
    if input.case_id.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField { field: "case_id" });
    }
    if input.comparison_profile_revision.trim().is_empty() {
        return Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision",
        });
    }

    let mut eligible_metrics = BTreeSet::new();
    let mut failure_flags = BTreeSet::new();
    let mut principal_expected = None;
    let mut principal_dispatched = None;

    let (expected_outcome, observed_outcome) = match input.scenario {
        FakeProviderScenario::Freshness {
            continuity,
            reconciliation,
            accepted_as_fresh,
        } => {
            eligible_metrics.insert(LabMetricKind::Eoffr);
            let reconciled = matches!(
                reconciliation,
                Some(ReconciliationCompleteness::Established)
            );
            let event_authority_is_continuous = continuity == EventContinuityState::Continuous;
            if accepted_as_fresh && !event_authority_is_continuous && !reconciled {
                failure_flags.insert(LabFailureFlag::EventOnlyFalseFreshness);
            }
            (
                "freshness_requires_continuity_or_established_reconciliation".to_owned(),
                if accepted_as_fresh {
                    "accepted_as_fresh".to_owned()
                } else {
                    "blocked_or_unresolved".to_owned()
                },
            )
        }
        FakeProviderScenario::Reconciliation {
            completeness,
            accepted_as_reconciled,
        } => {
            eligible_metrics.insert(LabMetricKind::Rmr);
            if accepted_as_reconciled && completeness != ReconciliationCompleteness::Established {
                failure_flags.insert(LabFailureFlag::ReconciliationMiss);
            }
            (
                "reconciliation_requires_established_completeness".to_owned(),
                if accepted_as_reconciled {
                    "accepted_as_reconciled".to_owned()
                } else {
                    "rejected_or_unresolved".to_owned()
                },
            )
        }
        FakeProviderScenario::ProviderAba {
            previous_provider_incarnation,
            current_provider_incarnation,
            opaque_provider_element_id,
            accepted_previous_identity_as_current,
        } => {
            if previous_provider_incarnation.as_str().trim().is_empty() {
                return Err(LabError::EmptyAuthorityField {
                    field: "previous_provider_incarnation",
                });
            }
            if current_provider_incarnation.as_str().trim().is_empty() {
                return Err(LabError::EmptyAuthorityField {
                    field: "current_provider_incarnation",
                });
            }
            if opaque_provider_element_id.trim().is_empty() {
                return Err(LabError::EmptyAuthorityField {
                    field: "opaque_provider_element_id",
                });
            }
            if previous_provider_incarnation == current_provider_incarnation {
                return Err(LabError::InvalidFakeProviderScenario {
                    reason: "provider_aba_requires_distinct_incarnations",
                });
            }

            eligible_metrics.insert(LabMetricKind::Piaer);
            if accepted_previous_identity_as_current {
                failure_flags.insert(LabFailureFlag::ProviderIdAbaEscape);
            }
            (
                format!(
                    "reject:{}@{}",
                    opaque_provider_element_id,
                    previous_provider_incarnation.as_str()
                ),
                if accepted_previous_identity_as_current {
                    format!(
                        "accepted_previous_identity_as_current@{}",
                        current_provider_incarnation.as_str()
                    )
                } else {
                    format!(
                        "rejected_previous_identity@{}",
                        current_provider_incarnation.as_str()
                    )
                },
            )
        }
        FakeProviderScenario::PrincipalDispatch {
            expected_principal,
            dispatched_principal,
        } => {
            validate_principal("expected_principal", &expected_principal)?;
            validate_principal("dispatched_principal", &dispatched_principal)?;
            eligible_metrics.insert(LabMetricKind::Wpdr);
            if expected_principal != dispatched_principal {
                failure_flags.insert(LabFailureFlag::WrongPrincipalDispatch);
            }
            principal_expected = Some(expected_principal.as_str().to_owned());
            principal_dispatched = Some(dispatched_principal.as_str().to_owned());
            (
                expected_principal.into_inner(),
                dispatched_principal.into_inner(),
            )
        }
        FakeProviderScenario::PrincipalVisibility {
            authorized_principal,
            exposed_principal,
        } => {
            validate_principal("authorized_principal", &authorized_principal)?;
            validate_principal("exposed_principal", &exposed_principal)?;
            eligible_metrics.insert(LabMetricKind::Pilr);
            if authorized_principal != exposed_principal {
                failure_flags.insert(LabFailureFlag::PrincipalInformationLeak);
            }
            principal_expected = Some(authorized_principal.as_str().to_owned());
            principal_dispatched = Some(exposed_principal.as_str().to_owned());
            (
                authorized_principal.into_inner(),
                exposed_principal.into_inner(),
            )
        }
    };

    let result_evidence = if failure_flags.is_empty() {
        ResultEvidence::PreregisteredSeedPass
    } else {
        ResultEvidence::CounterexampleFound
    };

    Ok(FakeProviderLabRecord {
        observation: LabObservation {
            observation_id: format!("fake_provider:{}", input.case_id),
            seed_id: None,
            expected_outcome,
            observed_outcome,
            principal_expected,
            principal_dispatched,
            eligible_metrics,
            failure_flags,
            evidence_refs: input.evidence_refs,
            provider_backed: true,
            comparison_profile_revision: input.comparison_profile_revision.to_owned(),
            logical_sequence: input.logical_sequence,
        },
        result_evidence,
    })
}

fn validate_principal(field: &'static str, principal: &PrincipalRef) -> Result<(), LabError> {
    if principal.as_str().trim().is_empty() {
        return Err(LabError::EmptyAuthorityField { field });
    }
    Ok(())
}
