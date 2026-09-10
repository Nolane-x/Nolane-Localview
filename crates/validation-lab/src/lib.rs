#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

impl ResearchResultClass {
    pub const ALL: [Self; 11] = [
        Self::ExploratoryObservation,
        Self::PreregisteredSeedPass,
        Self::CounterexampleFound,
        Self::NoCounterexampleWithinBoundN,
        Self::MutantKilled,
        Self::MutantSurvived,
        Self::DifferentialEquivalentWithinVectorSet,
        Self::DifferentialDivergenceFound,
        Self::PropertyCampaignPassN,
        Self::RealProviderIntegrationPass,
        Self::IndependentReplicationPass,
    ];
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonMode {
    Exact,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabRevisionIdentity {
    pub lab_revision: String,
    pub seed_corpus_revision: String,
    pub spec_revision_digest: String,
    pub reference_reducer_revision: String,
    pub mutation_catalog_revision: String,
    pub comparison_profile_revision: String,
    pub random_source_profile: String,
    pub platform_profile: Option<String>,
    pub start_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LabPreregistration {
    pub identity: LabRevisionIdentity,
    pub model_bound: String,
    pub random_seed: u64,
    pub comparison_rule_revision: String,
    pub expected_distinction: String,
    pub seed_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabSeed {
    pub seed_id: String,
    pub family: String,
    pub spec_surface_refs: Vec<u64>,
    pub input_fixture: Value,
    pub expected_semantic_outcome: String,
    pub forbidden_outcomes: BTreeSet<String>,
    pub comparison_mode: ComparisonMode,
    pub risk_if_missed: RiskLevel,
    pub prediction_revision: String,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum LabMetricKind {
    Suar,
    Wpdr,
    Pilr,
    Eoffr,
    Rmr,
    Piaer,
    Wfir,
    Pdmr,
    Scar,
    Uobrr,
    Msr,
    Crdr,
    Rpomr,
    Cbfr,
}

impl LabMetricKind {
    pub const ALL: [Self; 14] = [
        Self::Suar,
        Self::Wpdr,
        Self::Pilr,
        Self::Eoffr,
        Self::Rmr,
        Self::Piaer,
        Self::Wfir,
        Self::Pdmr,
        Self::Scar,
        Self::Uobrr,
        Self::Msr,
        Self::Crdr,
        Self::Rpomr,
        Self::Cbfr,
    ];

    pub const SILENT_UNSOUNDNESS_ZERO_TARGETS: [Self; 5] = [
        Self::Suar,
        Self::Wpdr,
        Self::Pilr,
        Self::Pdmr,
        Self::Uobrr,
    ];
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MetricMeasurementStatus {
    NotMeasured,
    Measured,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricEvent {
    pub kind: LabMetricKind,
    pub eligible: bool,
    pub violation: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricSummary {
    pub numerator: u64,
    pub denominator: u64,
    pub status: MetricMeasurementStatus,
}

impl MetricSummary {
    pub fn rate(&self) -> Option<f64> {
        if self.denominator == 0 {
            None
        } else {
            Some(self.numerator as f64 / self.denominator as f64)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SilentUnsoundnessVerdict {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabSeedResult {
    pub seed_id: String,
    pub used_preregistration_digest: Option<String>,
    pub expected_preregistration_digest: String,
    pub observed_semantic_outcome: String,
    pub result_class: ResearchResultClass,
    pub metric_events: Vec<MetricEvent>,
    pub provenance_ids: Vec<String>,
}

#[derive(Debug, Error)]
pub enum LabEncodingError {
    #[error("failed to encode deterministic lab record: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn aggregate_metrics(events: &[MetricEvent]) -> BTreeMap<LabMetricKind, MetricSummary> {
    let mut summaries = LabMetricKind::ALL
        .into_iter()
        .map(|kind| {
            (
                kind,
                MetricSummary {
                    numerator: 0,
                    denominator: 0,
                    status: MetricMeasurementStatus::NotMeasured,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for event in events {
        if !event.eligible {
            continue;
        }
        let summary = summaries
            .get_mut(&event.kind)
            .expect("all metric kinds are initialized");
        summary.denominator = summary.denominator.saturating_add(1);
        if event.violation {
            summary.numerator = summary.numerator.saturating_add(1);
        }
        summary.status = MetricMeasurementStatus::Measured;
    }

    summaries
}

pub fn silent_unsoundness_verdict(
    summaries: &BTreeMap<LabMetricKind, MetricSummary>,
) -> SilentUnsoundnessVerdict {
    let mut has_unmeasured = false;

    for kind in LabMetricKind::SILENT_UNSOUNDNESS_ZERO_TARGETS {
        let Some(summary) = summaries.get(&kind) else {
            has_unmeasured = true;
            continue;
        };
        if summary.status == MetricMeasurementStatus::Measured && summary.numerator > 0 {
            return SilentUnsoundnessVerdict::Fail;
        }
        if summary.status != MetricMeasurementStatus::Measured || summary.denominator == 0 {
            has_unmeasured = true;
        }
    }

    if has_unmeasured {
        SilentUnsoundnessVerdict::Inconclusive
    } else {
        SilentUnsoundnessVerdict::Pass
    }
}

pub fn classify_seed_result(
    passed: bool,
    used_preregistration_digest: Option<&str>,
    expected_preregistration_digest: &str,
) -> ResearchResultClass {
    if !passed {
        return ResearchResultClass::CounterexampleFound;
    }

    match used_preregistration_digest {
        Some(used)
            if !expected_preregistration_digest.is_empty()
                && used == expected_preregistration_digest =>
        {
            ResearchResultClass::PreregisteredSeedPass
        }
        _ => ResearchResultClass::ExploratoryObservation,
    }
}

pub fn preregistration_digest(
    value: &LabPreregistration,
) -> Result<String, LabEncodingError> {
    deterministic_digest(value)
}

pub fn result_digest(value: &LabSeedResult) -> Result<String, LabEncodingError> {
    deterministic_digest(value)
}

fn deterministic_digest<T: Serialize>(value: &T) -> Result<String, LabEncodingError> {
    let encoded = serde_json::to_vec(value)?;
    let digest = Sha256::digest(encoded);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
