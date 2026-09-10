#![forbid(unsafe_code)]

mod canonical;
mod identity;
mod metrics;
mod preregistration;
mod seeds;

pub use canonical::{CanonicalDigest, canonical_digest, canonical_json_bytes};
pub use identity::LabRevisionContext;
pub use metrics::{
    LabMetricKind, LabMetricValue, MetricSnapshot, MetricStatus, SilentUnsoundnessGateStatus,
    evaluate_silent_unsoundness_gate,
};
pub use preregistration::{
    CampaignLayer, LabPreregistration, PersistedPreregistrationReceipt, PreparedPreregistration,
    ValidatedPreregistrationReceipt, validate_persisted_receipt,
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
    #[error("canonical serialization failed: {message}")]
    CanonicalSerialization { message: String },
    #[error("persisted preregistration digest does not match prepared preregistration")]
    PreregistrationPersistenceMismatch {
        expected: CanonicalDigest,
        actual: CanonicalDigest,
    },
    #[error("invalid persisted preregistration receipt: {reason}")]
    InvalidPersistenceReceipt { reason: &'static str },
}
