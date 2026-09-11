use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CanonicalDigest, CompletedLabRunIdentity, LabError, LabMetricKind, LabObservation,
    LabPreregistration, LabRevisionContext, LabSeedIdentity, MetricSnapshot, MetricStatus,
    PreregistrationReceiptProjection, ProviderCampaignKind, ValidatedPreregistrationReceipt,
    canonical_digest, reduce_metric_observations, validate_provider_campaign_layer,
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
    declared_metrics: Option<BTreeSet<LabMetricKind>>,
    observations: Vec<LabObservation>,
    observation_digests: Vec<CanonicalDigest>,
    last_observation_sequence: Option<u64>,
    actual_execution_authority: ActualExecutionAuthority,
    assumptions_used: BTreeSet<String>,
    bound_used: Option<u64>,
    provider_campaign_kind: Option<ProviderCampaignKind>,
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
                    declared_metrics: Some(preregistration.declared_metrics),
                    observations: Vec::new(),
                    observation_digests: Vec::new(),
                    last_observation_sequence: None,
                    actual_execution_authority,
                    assumptions_used: preregistration.assumptions,
                    bound_used: preregistration.model_bound,
                    provider_campaign_kind: None,
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
                    declared_metrics: None,
                    observations: Vec::new(),
                    observation_digests: Vec::new(),
                    last_observation_sequence: None,
                    actual_execution_authority,
                    assumptions_used: assumptions,
                    bound_used: None,
                    provider_campaign_kind: None,
                    finalized: false,
                })
            }
        }
    }

    pub fn start_provider_campaign(
        campaign: ProviderCampaignKind,
        admission: LabRunAdmission,
        actual_execution_authority: ActualExecutionAuthority,
    ) -> Result<Self, LabError> {
        let layer = match &admission {
            LabRunAdmission::Prospective {
                preregistration, ..
            } => preregistration.campaign_layer,
            LabRunAdmission::Exploratory { .. } => {
                return Err(LabError::ProviderCampaignRequiresProspectiveAdmission);
            }
        };
        validate_provider_campaign_layer(campaign, layer)?;

        let mut builder = Self::start(admission, actual_execution_authority)?;
        builder.provider_campaign_kind = Some(campaign);
        Ok(builder)
    }

    pub fn append_observation(&mut self, observation: LabObservation) -> Result<(), LabError> {
        if self.finalized {
            return Err(LabError::AlreadyFinalized);
        }

        if observation.comparison_profile_revision
            != self.actual_execution_authority.comparison_profile_revision
        {
            return Err(LabError::ObservationAuthorityDrift {
                field: "comparison_profile_revision",
            });
        }

        let start_sequence = self.revision_context.start_sequence;
        if observation.logical_sequence <= start_sequence {
            return Err(LabError::ObservationSequenceNotAfterStart {
                logical_sequence: observation.logical_sequence,
                start_sequence,
            });
        }
        if let Some(previous_sequence) = self.last_observation_sequence {
            if observation.logical_sequence <= previous_sequence {
                return Err(LabError::ObservationSequenceNotMonotonic {
                    previous_sequence,
                    logical_sequence: observation.logical_sequence,
                });
            }
        }

        if let ExecutionMode::Prospective { .. } = self.execution_mode {
            if observation.provider_backed && self.revision_context.platform_profile.is_none() {
                return Err(LabError::ProviderBackedObservationRequiresPlatformProfile);
            }
            if let Some(declared_metrics) = &self.declared_metrics {
                if let Some(metric) = observation
                    .eligible_metrics
                    .iter()
                    .find(|metric| !declared_metrics.contains(metric))
                {
                    return Err(LabError::ObservationUsesUndeclaredMetric { metric: *metric });
                }
            }
        }

        let digest = canonical_digest(&observation)?;
        self.last_observation_sequence = Some(observation.logical_sequence);
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

        let metric_snapshot = reduce_metric_observations(&self.observations)?;
        if evidence == ResultEvidence::RealProviderIntegrationPass {
            self.validate_real_provider_pass(&metric_snapshot)?;
        }

        let result_class = match self.execution_mode {
            ExecutionMode::Prospective { .. } => evidence.class(),
            ExecutionMode::Exploratory { .. } => ResearchResultClass::ExploratoryObservation,
        };

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

    fn validate_real_provider_pass(&self, snapshot: &MetricSnapshot) -> Result<(), LabError> {
        if self.provider_campaign_kind != Some(ProviderCampaignKind::RealProviderSeedApplications) {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_typed_l7_campaign_admission",
            });
        }
        if self.revision_context.platform_profile.is_none() {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_platform_profile",
            });
        }

        let rpomr = snapshot
            .get(LabMetricKind::Rpomr)
            .expect("RPOMR is always initialized in a metric snapshot");
        if rpomr.status != MetricStatus::Measured || rpomr.denominator == 0 {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_measured_rpomr",
            });
        }
        if rpomr.numerator != 0 {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_zero_rpomr_mismatches",
            });
        }
        if self.observations.iter().any(|observation| {
            !observation.provider_backed
                || !observation.eligible_metrics.contains(&LabMetricKind::Rpomr)
        }) {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_provider_backed_rpomr_observations",
            });
        }
        if self
            .observations
            .iter()
            .any(|observation| !observation.failure_flags.is_empty())
        {
            return Err(LabError::InvalidRealProviderPass {
                reason: "real_provider_pass_requires_failure_free_observations",
            });
        }
        Ok(())
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
