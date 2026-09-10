//! Deterministic research authority for LocalView V4.3 validation campaigns.
//!
//! A validated preregistration receipt is an authority capability minted only by
//! `validate_persisted_receipt`; untrusted serialized data must not be able to
//! reconstruct it directly.
//!
//! ```compile_fail
//! fn require_deserialize<T: serde::de::DeserializeOwned>() {}
//! require_deserialize::<localview_validation_lab::ValidatedPreregistrationReceipt>();
//! ```

#![forbid(unsafe_code)]

mod artifact;
mod canonical;
mod identity;
mod metrics;
mod mutation_adapter;
mod preregistration;
mod result;
mod seeds;
mod semantic_adapter;

pub use artifact::{CanonicalArtifact, LabArtifactKind, LabArtifactState};
pub use canonical::{CanonicalDigest, canonical_digest, canonical_json_bytes};
pub use identity::{CompletedLabRunIdentity, LabRevisionContext};
pub use metrics::{
    LabFailureFlag, LabMetricKind, LabMetricValue, LabObservation, MetricSnapshot, MetricStatus,
    SilentUnsoundnessGateStatus, evaluate_silent_unsoundness_gate, reduce_metric_observations,
};
pub use mutation_adapter::{
    MutationLabRecord, MutationNotMeasuredReason, adapt_mutation_outcome,
};
pub use preregistration::{
    CampaignLayer, LabPreregistration, PersistedPreregistrationReceipt, PreparedPreregistration,
    PreregistrationReceiptProjection, ValidatedPreregistrationReceipt, validate_persisted_receipt,
};
pub use result::{
    ActualExecutionAuthority, CompletedLabRun, DowngradeReason, ExecutionMode, LabResultPayload,
    LabRunAdmission, LabRunBuilder, ResearchResultClass, ResultEvidence,
};
pub use seeds::{LabSeed, LabSeedCatalog, LabSeedIdentity};
pub use semantic_adapter::{SemanticSeedLabRecord, adapt_semantic_seed_outcome};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LabError {
    #[error("metric numerator {numerator} exceeds denominator {denominator}")]
    InvalidMetricSubset { numerator: u64, denominator: u64 },
    #[error("metric rate could not be represented as u64 parts-per-billion")]
    MetricRateOverflow,
    #[error("metric {kind:?} {counter} counter overflowed")]
    MetricCounterOverflow {
        kind: LabMetricKind,
        counter: &'static str,
    },
    #[error("failure flag {flag:?} requires eligibility for metric {metric:?}")]
    FailureFlagWithoutEligibility {
        flag: LabFailureFlag,
        metric: LabMetricKind,
    },
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
    #[error("unsupported L1 semantic comparison mode: {mode}")]
    UnsupportedSemanticComparisonMode { mode: String },
    #[error("canonical serialization failed: {message}")]
    CanonicalSerialization { message: String },
    #[error("persisted preregistration digest does not match prepared preregistration")]
    PreregistrationPersistenceMismatch {
        expected: CanonicalDigest,
        actual: CanonicalDigest,
    },
    #[error("invalid persisted preregistration receipt: {reason}")]
    InvalidPersistenceReceipt { reason: &'static str },
    #[error(
        "preregistration receipt sequence {receipt_sequence} is not before run start sequence {start_sequence}"
    )]
    PreregistrationNotPersistedBeforeStart {
        receipt_sequence: u64,
        start_sequence: u64,
    },
    #[error("prospective execution authority drifted at {field}")]
    ProspectiveAuthorityDrift { field: &'static str },
    #[error("provider-backed prospective observation requires a preregistered platform profile")]
    ProviderBackedObservationRequiresPlatformProfile,
    #[error("observation authority drifted at {field}")]
    ObservationAuthorityDrift { field: &'static str },
    #[error(
        "observation sequence {logical_sequence} must be after run start sequence {start_sequence}"
    )]
    ObservationSequenceNotAfterStart {
        logical_sequence: u64,
        start_sequence: u64,
    },
    #[error(
        "observation sequence {logical_sequence} must be greater than previous accepted sequence {previous_sequence}"
    )]
    ObservationSequenceNotMonotonic {
        previous_sequence: u64,
        logical_sequence: u64,
    },
    #[error("prospective observation uses undeclared metric {metric:?}")]
    ObservationUsesUndeclaredMetric { metric: LabMetricKind },
    #[error("validation lab run is already finalized")]
    AlreadyFinalized,
}
