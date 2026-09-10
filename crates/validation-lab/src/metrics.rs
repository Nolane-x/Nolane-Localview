use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
