use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CanonicalDigest, CompletedLabRunIdentity, LabError, LabObservation, LabPreregistration,
    LabRevisionContext, LabSeedIdentity, MetricSnapshot, PreregistrationReceiptProjection,
    ValidatedPreregistrationReceipt, canonical_digest, reduce_metric_observations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchResultClass {
    ExploratoryObservation,
    PreregisteredSeedPass,
    CounterexampleFound,
    NoCounterexampleWithinBoundN,
    MutantKilled,
    MutantSurvived,
    DifferentialEquivalentWithinVectorSet,
    DifferentialDivergenceFound,
    PropertyCampaignPassN,
    RealProviderIntegrationPass,
    IndependentReplicationPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultEvidence {
    ExploratoryObservation,
    PreregisteredSeedPass,
    CounterexampleFound,
    NoCounterexampleWithinBoundN,
    MutantKilled,
    MutantSurvived,
    DifferentialEquivalentWithinVectorSet,
    DifferentialDivergenceFound,
    PropertyCampaignPassN,
    RealProviderIntegrationPass,
    IndependentReplicationPass,
}

impl ResultEvidence {
    pub fn class(self) -> ResearchResultClass {
        match self {
            Self::ExploratoryObservation => ResearchResultClass::ExploratoryObservation,
            Self::PreregisteredSeedPass => ResearchResultClass::PreregisteredSeedPass,
            Self::CounterexampleFound => ResearchResultClass::CounterexampleFound,
            Self::NoCounterexampleWithinBoundN => ResearchResultClass::NoCounterexampleWithinBoundN,
            Self::MutantKilled => ResearchResultClass::MutantKilled,
            Self::MutantSurvived => ResearchResultClass::MutantSurvived,
            Self::DifferentialEquivalentWithinVectorSet => {
                ResearchResultClass::DifferentialEquivalentWithinVectorSet
            }
            Self::DifferentialDivergenceFound => ResearchResultClass::DifferentialDivergenceFound,
            Self::PropertyCampaignPassN => ResearchResultClass::PropertyCampaignPassN,
            Self::RealProviderIntegrationPass => ResearchResultClass::RealProviderIntegrationPass,
            Self::IndependentReplicationPass => ResearchResultClass::IndependentReplicationPass,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DowngradeReason {
    MissingPersistedPreregistration,
    PreregistrationPreparationFailed,
    CallerSelectedExploratory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActualExecutionAuthority {
    pub seed_catalog_digest: CanonicalDigest,
    pub comparison_profile_revision: String,
    pub random_source_profile: String,
    pub model_bound: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabRunAdmission {
    Prospective {
        preregistration: LabPreregistration,
        receipt: ValidatedPreregistrationReceipt,
    },
    Exploratory {
        revision_context: LabRevisionContext,
        seed_identities: Vec<LabSeedIdentity>,
        assumptions: BTreeSet<String>,
        downgrade_reason: DowngradeReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum ExecutionMode {
    Prospective {
        receipt: PreregistrationReceiptProjection,
    },
    Exploratory {
        downgrade_reason: DowngradeReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabResultPayload {
    pub preregistration_digest: Option<CanonicalDigest>,
    pub revision_context: LabRevisionContext,
    pub execution_mode: ExecutionMode,
    pub result_class: ResearchResultClass,
    pub submitted_evidence: ResultEvidence,
    pub seed_identities: Vec<LabSeedIdentity>,
    pub observation_digests: Vec<CanonicalDigest>,
    pub metric_snapshot: MetricSnapshot,
    pub actual_execution_authority: ActualExecutionAuthority,
    pub assumptions_used: BTreeSet<String>,
    pub bound_used: Option<u64>,
    pub start_sequence: u64,
    pub end_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedLabRun {
    pub identity: CompletedLabRunIdentity,
    pub payload: LabResultPayload,
}

#[derive(Debug)]
pub struct LabRunBuilder {
    preregistration_digest: Option<CanonicalDigest>,
    revision_context: LabRevisionContext,
    execution_mode: ExecutionMode,
    seed_identities: Vec<LabSeedIdentity>,
    observations: Vec<LabObservation>,
    observation_digests: Vec<CanonicalDigest>,
    actual_execution_authority: ActualExecutionAuthority,
    assumptions_used: BTreeSet<String>,
    bound_used: Option<u64>,
    finalized: bool,
}

impl LabRunBuilder {
    pub fn start(
        admission: LabRunAdmission,
        actual_execution_authority: ActualExecutionAuthority,
    ) -> Result<Self, LabError> {
        match admission {
            LabRunAdmission::Prospective {
                preregistration,
                receipt,
            } => {
                let prepared = preregistration.prepare()?;
                if receipt.digest() != &prepared.digest {
                    return Err(LabError::ProspectiveAuthorityDrift {
                        field: "preregistration_digest",
                    });
                }

                let start_sequence = preregistration.revision_context.start_sequence;
                if receipt.logical_sequence() >= start_sequence {
                    return Err(LabError::PreregistrationNotPersistedBeforeStart {
                        receipt_sequence: receipt.logical_sequence(),
                        start_sequence,
                    });
                }

                validate_actual_authority(&preregistration, &actual_execution_authority)?;

                Ok(Self {
                    preregistration_digest: Some(prepared.digest),
                    revision_context: preregistration.revision_context,
                    execution_mode: ExecutionMode::Prospective {
                        receipt: PreregistrationReceiptProjection::from(&receipt),
                    },
                    seed_identities: preregistration.seed_identities,
                    observations: Vec::new(),
                    observation_digests: Vec::new(),
                    actual_execution_authority,
                    assumptions_used: preregistration.assumptions,
                    bound_used: preregistration.model_bound,
                    finalized: false,
                })
            }
            LabRunAdmission::Exploratory {
                revision_context,
                seed_identities,
                assumptions,
                downgrade_reason,
            } => {
                revision_context.validate()?;
                for identity in &seed_identities {
                    identity.validate()?;
                }

                Ok(Self {
                    preregistration_digest: None,
                    revision_context,
                    execution_mode: ExecutionMode::Exploratory { downgrade_reason },
                    seed_identities,
                    observations: Vec::new(),
                    observation_digests: Vec::new(),
                    actual_execution_authority,
                    assumptions_used: assumptions,
                    bound_used: None,
                    finalized: false,
                })
            }
        }
    }

    pub fn append_observation(&mut self, observation: LabObservation) -> Result<(), LabError> {
        if self.finalized {
            return Err(LabError::AlreadyFinalized);
        }
        let digest = canonical_digest(&observation)?;
        self.observations.push(observation);
        self.observation_digests.push(digest);
        Ok(())
    }

    pub fn finalize(
        &mut self,
        evidence: ResultEvidence,
        end_sequence: u64,
    ) -> Result<CompletedLabRun, LabError> {
        if self.finalized {
            return Err(LabError::AlreadyFinalized);
        }

        let result_class = match self.execution_mode {
            ExecutionMode::Prospective { .. } => evidence.class(),
            ExecutionMode::Exploratory { .. } => ResearchResultClass::ExploratoryObservation,
        };
        let metric_snapshot = reduce_metric_observations(&self.observations)?;

        let payload = LabResultPayload {
            preregistration_digest: self.preregistration_digest.clone(),
            revision_context: self.revision_context.clone(),
            execution_mode: self.execution_mode.clone(),
            result_class,
            submitted_evidence: evidence,
            seed_identities: self.seed_identities.clone(),
            observation_digests: self.observation_digests.clone(),
            metric_snapshot,
            actual_execution_authority: self.actual_execution_authority.clone(),
            assumptions_used: self.assumptions_used.clone(),
            bound_used: self.bound_used,
            start_sequence: self.revision_context.start_sequence,
            end_sequence,
        };
        let result_artifact_digest = canonical_digest(&payload)?;
        let identity = CompletedLabRunIdentity {
            revision_context: self.revision_context.clone(),
            result_artifact_digest,
        };

        self.finalized = true;
        Ok(CompletedLabRun { identity, payload })
    }
}

fn validate_actual_authority(
    preregistration: &LabPreregistration,
    actual: &ActualExecutionAuthority,
) -> Result<(), LabError> {
    if actual.seed_catalog_digest != preregistration.seed_catalog_digest {
        return Err(LabError::ProspectiveAuthorityDrift {
            field: "seed_catalog_digest",
        });
    }
    if actual.comparison_profile_revision
        != preregistration
            .revision_context
            .comparison_profile_revision
    {
        return Err(LabError::ProspectiveAuthorityDrift {
            field: "comparison_profile_revision",
        });
    }
    if actual.random_source_profile != preregistration.revision_context.random_source_profile {
        return Err(LabError::ProspectiveAuthorityDrift {
            field: "random_source_profile",
        });
    }
    if actual.model_bound != preregistration.model_bound {
        return Err(LabError::ProspectiveAuthorityDrift {
            field: "model_bound",
        });
    }
    Ok(())
}
