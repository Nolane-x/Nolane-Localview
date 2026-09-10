#![forbid(unsafe_code)]

mod metrics;
mod seeds;

pub use metrics::{
    LabMetricKind, LabMetricValue, MetricSnapshot, MetricStatus, SilentUnsoundnessGateStatus,
    evaluate_silent_unsoundness_gate,
};
pub use seeds::{LabSeed, LabSeedCatalog, LabSeedIdentity};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LabError {
    #[error("metric numerator {numerator} exceeds denominator {denominator}")]
    InvalidMetricSubset { numerator: u64, denominator: u64 },
    #[error("metric rate could not be represented as u64 parts-per-billion")]
    MetricRateOverflow,
    #[error(
        "duplicate seed identity: seed_id={seed_id}, prediction_revision={prediction_revision}, oracle_revision={oracle_revision}"
    )]
    DuplicateSeedIdentity {
        seed_id: String,
        prediction_revision: String,
        oracle_revision: String,
    },
    #[error("authority field {field} cannot be empty")]
    EmptyAuthorityField { field: &'static str },
}
