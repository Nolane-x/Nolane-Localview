use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::LabError;

const RATE_SCALE_PPB: u128 = 1_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

    pub const SILENT_UNSOUNDNESS_ZERO_TARGET: [Self; 5] = [
        Self::Suar,
        Self::Wpdr,
        Self::Pilr,
        Self::Pdmr,
        Self::Uobrr,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabFailureFlag {
    SilentUnsoundAction,
    WrongPrincipalDispatch,
    PrincipalInformationLeak,
    EventOnlyFalseFreshness,
    ReconciliationMiss,
    ProviderIdAbaEscape,
    WrongForegroundInput,
    PartialDispatchMisclassifiedSuccess,
    StaleCacheAuthority,
    BlindRetryAfterUnknown,
    MutationSurvived,
    CrossReducerDivergence,
    RealProviderOracleMismatch,
    CleanupToBaselineFailure,
}

impl LabFailureFlag {
    pub const fn metric(self) -> LabMetricKind {
        match self {
            Self::SilentUnsoundAction => LabMetricKind::Suar,
            Self::WrongPrincipalDispatch => LabMetricKind::Wpdr,
            Self::PrincipalInformationLeak => LabMetricKind::Pilr,
            Self::EventOnlyFalseFreshness => LabMetricKind::Eoffr,
            Self::ReconciliationMiss => LabMetricKind::Rmr,
            Self::ProviderIdAbaEscape => LabMetricKind::Piaer,
            Self::WrongForegroundInput => LabMetricKind::Wfir,
            Self::PartialDispatchMisclassifiedSuccess => LabMetricKind::Pdmr,
            Self::StaleCacheAuthority => LabMetricKind::Scar,
            Self::BlindRetryAfterUnknown => LabMetricKind::Uobrr,
            Self::MutationSurvived => LabMetricKind::Msr,
            Self::CrossReducerDivergence => LabMetricKind::Crdr,
            Self::RealProviderOracleMismatch => LabMetricKind::Rpomr,
            Self::CleanupToBaselineFailure => LabMetricKind::Cbfr,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabObservation {
    pub observation_id: String,
    pub seed_id: Option<String>,
    pub expected_outcome: String,
    pub observed_outcome: String,
    pub principal_expected: Option<String>,
    pub principal_dispatched: Option<String>,
    pub eligible_metrics: BTreeSet<LabMetricKind>,
    pub failure_flags: BTreeSet<LabFailureFlag>,
    pub evidence_refs: BTreeSet<String>,
    pub provider_backed: bool,
    pub comparison_profile_revision: String,
    pub logical_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricStatus {
    Measured,
    NotMeasured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabMetricValue {
    pub kind: LabMetricKind,
    pub numerator: u64,
    pub denominator: u64,
    pub status: MetricStatus,
    pub rate_ppb: Option<u64>,
}

impl LabMetricValue {
    pub fn new(kind: LabMetricKind, numerator: u64, denominator: u64) -> Result<Self, LabError> {
        if numerator > denominator {
            return Err(LabError::InvalidMetricSubset {
                numerator,
                denominator,
            });
        }

        if denominator == 0 {
            return Ok(Self {
                kind,
                numerator,
                denominator,
                status: MetricStatus::NotMeasured,
                rate_ppb: None,
            });
        }

        let rate = (u128::from(numerator) * RATE_SCALE_PPB) / u128::from(denominator);
        let rate_ppb = u64::try_from(rate).map_err(|_| LabError::MetricRateOverflow)?;

        Ok(Self {
            kind,
            numerator,
            denominator,
            status: MetricStatus::Measured,
            rate_ppb: Some(rate_ppb),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricSnapshot {
    values: BTreeMap<LabMetricKind, LabMetricValue>,
}

impl MetricSnapshot {
    pub fn empty() -> Self {
        let values = LabMetricKind::ALL
            .into_iter()
            .map(|kind| {
                let value = LabMetricValue::new(kind, 0, 0)
                    .expect("zero-over-zero metric construction is always valid");
                (kind, value)
            })
            .collect();
        Self { values }
    }

    pub fn get(&self, kind: LabMetricKind) -> Option<&LabMetricValue> {
        self.values.get(&kind)
    }

    pub fn set(&mut self, value: LabMetricValue) {
        self.values.insert(value.kind, value);
    }
}

impl Default for MetricSnapshot {
    fn default() -> Self {
        Self::empty()
    }
}

pub fn reduce_metric_observations(
    observations: &[LabObservation],
) -> Result<MetricSnapshot, LabError> {
    for observation in observations {
        for flag in &observation.failure_flags {
            let metric = flag.metric();
            if !observation.eligible_metrics.contains(&metric) {
                return Err(LabError::FailureFlagWithoutEligibility {
                    flag: *flag,
                    metric,
                });
            }
        }
    }

    let mut counters = LabMetricKind::ALL
        .into_iter()
        .map(|kind| (kind, (0_u64, 0_u64)))
        .collect::<BTreeMap<_, _>>();

    for observation in observations {
        for metric in &observation.eligible_metrics {
            let (_, denominator) = counters
                .get_mut(metric)
                .expect("all V4.3 metric counters are initialized");
            *denominator = denominator.checked_add(1).ok_or(LabError::MetricCounterOverflow {
                kind: *metric,
                counter: "denominator",
            })?;
        }

        for flag in &observation.failure_flags {
            let metric = flag.metric();
            let (numerator, _) = counters
                .get_mut(&metric)
                .expect("all V4.3 metric counters are initialized");
            *numerator = numerator.checked_add(1).ok_or(LabError::MetricCounterOverflow {
                kind: metric,
                counter: "numerator",
            })?;
        }
    }

    let mut snapshot = MetricSnapshot::empty();
    for (kind, (numerator, denominator)) in counters {
        snapshot.set(LabMetricValue::new(kind, numerator, denominator)?);
    }
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilentUnsoundnessGateStatus {
    Pass,
    Fail,
    Incomplete,
}

pub fn evaluate_silent_unsoundness_gate(
    snapshot: &MetricSnapshot,
) -> SilentUnsoundnessGateStatus {
    let mut incomplete = false;

    for kind in LabMetricKind::SILENT_UNSOUNDNESS_ZERO_TARGET {
        let Some(value) = snapshot.get(kind) else {
            incomplete = true;
            continue;
        };

        match value.status {
            MetricStatus::NotMeasured => incomplete = true,
            MetricStatus::Measured if value.numerator > 0 => {
                return SilentUnsoundnessGateStatus::Fail;
            }
            MetricStatus::Measured => {}
        }
    }

    if incomplete {
        SilentUnsoundnessGateStatus::Incomplete
    } else {
        SilentUnsoundnessGateStatus::Pass
    }
}
