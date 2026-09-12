use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! semantic_ref {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

semantic_ref!(PrincipalRef);
semantic_ref!(ProviderIncarnationRef);
semantic_ref!(TargetIncarnationRef);

/// Whether the request crossed the transport boundary to the intended executor.
///
/// This is deliberately orthogonal to provider dispatch and world-state verification.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TransportResult {
    DeliveredToExecutor,
    RejectedBeforeExecutor,
    TransportTimeout,
    TransportDisconnected,
    TransportAuthFailed,
    TransportSchemaUnsupported,
}

/// What the provider/executor can establish about dispatch itself.
///
/// A full dispatch does not prove that the expected world outcome occurred.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DispatchResult {
    NotDispatched,
    DispatchedFull,
    DispatchedPartial,
    DispatchRejected,
    DispatchAmbiguous,
    DispatchBlockedPermission,
    DispatchBlockedIdentity,
    DispatchBlockedFocus,
    DispatchBlockedProvider,
}

/// The independently verified post-dispatch world-state result.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorldOutcome {
    VerifiedExpected,
    VerifiedUnexpected,
    FailedKnown,
    UnknownOutcome,
    ReconciliationRequired,
    CompensatedVerified,
    CompensationFailed,
}

/// Continuity of the accepted provider event lineage.
///
/// Reconnection is represented explicitly and never aliases continuity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EventContinuityState {
    Continuous,
    GapDetected,
    SequenceReset,
    ProviderReincarnated,
    OrderingOpaque,
    ReconciliationRequired,
    ReconnectedUnreconciled,
    Broken,
}

/// Whether a bounded reconciliation observation is sufficient to re-establish
/// the declared current semantic surface.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationCompleteness {
    Established,
    Incomplete,
    Inconclusive,
    Unsupported,
}

/// Provider-local realization state. Virtual provider objects are not promoted
/// to currently actionable realized elements by identity alone.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderElementRealization {
    DiscoveredVirtual,
    RealizationRequired,
    Realizing,
    RealizedCurrent,
    RealizationFailed,
    StaleAfterRealization,
}

/// Evidence that a fresh bounded observation re-established a declared current
/// semantic surface. This is intentionally separate from event continuity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReconciliationSnapshotReceipt {
    pub receipt_id: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub snapshot_cut_ref: String,
    pub surface_scope: String,
    pub completeness: ReconciliationCompleteness,
    pub cache_profile_revision: String,
    pub permission_visibility_revision: String,
    pub capture_sequence: u64,
    pub observed_digest: String,
    #[serde(default)]
    pub incompleteness_debt: Vec<String>,
}

/// A provider-local element identity that is meaningful only within explicit
/// provider and target incarnations. Opaque provider IDs are never durable
/// identity by themselves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderElementRef {
    pub provider_family: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub opaque_provider_element_id: String,
    #[serde(default)]
    pub semantic_locator_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_surface_ref: Option<String>,
    pub acquisition_cut_ref: String,
    pub realization: ProviderElementRealization,
    pub lifetime_profile_revision: String,
}
