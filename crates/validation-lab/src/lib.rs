#![forbid(unsafe_code)]

mod metrics;

pub use metrics::{
    LabMetricKind, LabMetricValue, MetricSnapshot, MetricStatus, SilentUnsoundnessGateStatus,
    evaluate_silent_unsoundness_gate,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LabError {
    #[error("metric numerator {numerator} exceeds denominator {denominator}")]
    InvalidMetricSubset { numerator: u64, denominator: u64 },
    #[error("metric rate could not be represented as u64 parts-per-billion")]
    MetricRateOverflow,
}
