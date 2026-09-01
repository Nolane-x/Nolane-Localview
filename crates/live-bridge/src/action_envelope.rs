use localview_protocol::{
    PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::BridgeAction;

/// Minimum side-effect/risk floor for a canonical action.
///
/// The class is intentionally ordinal only by policy convention; callers must not
/// cast it to a numeric score and use that as authority.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ActionRiskClass {
    ObserveOnly,
    ReversibleUiState,
    LocalDataMutation,
    ExternalSideEffect,
    DestructiveOrIrreversible,
    CredentialOrAuthorityChange,
    Unknown,
}

/// Retry/idempotency semantics are explicit because UNKNOWN_OUTCOME must not
/// silently become an automatic retry for consequential work.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ActionIdempotencyClass {
    Idempotent,
    NonIdempotent,
    Unknown,
}

/// Canonical authority metadata that lives above the compact BridgeAction wire
/// object. None of these fields may be reconstructed from transport success or
/// legacy `ok: bool` results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionEnvelopeMetadata {
    pub decision_principal_ref: PrincipalRef,
    pub acting_principal_ref: PrincipalRef,
    pub authorization_revision: String,
    pub precondition_snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub risk_class: ActionRiskClass,
    pub idempotency_class: ActionIdempotencyClass,
    #[serde(default)]
    pub expected_postcondition_contract_refs: Vec<String>,
}

/// Immutable in-memory canonical action binding for Repository Migration Phase 3.
///
/// Durability is intentionally deferred to Phase 4's consequential journal. The
/// transport action ID points to this envelope; the legacy BridgeAction schema is
/// left unchanged for V1-V3 compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalActionEnvelope {
    pub envelope_id: Uuid,
    pub transport_action_id: Uuid,
    pub session_id: SessionId,
    pub metadata: ActionEnvelopeMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalQueuedAction {
    pub action: BridgeAction,
    pub envelope: CanonicalActionEnvelope,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionEnvelopeBindingError {
    MissingProviderObservation,
    ProviderIncarnationMismatch,
    TargetIncarnationMismatch,
}
