use localview_protocol::{
    PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::BridgeAction;

/// Minimum side-effect/risk floor for a canonical action.
///
/// The V4 taxonomy is carried on the wire explicitly so policy does not infer
/// risk from a numeric score or from the transport action kind alone.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionRiskClass {
    #[serde(rename = "s0_observe_only")]
    ObserveOnly,
    #[serde(rename = "s1_reversible_ui_state")]
    ReversibleUiState,
    #[serde(rename = "s2_local_data_mutation")]
    LocalDataMutation,
    #[serde(rename = "s3_external_side_effect")]
    ExternalSideEffect,
    #[serde(rename = "s4_destructive_or_irreversible")]
    DestructiveOrIrreversible,
    #[serde(rename = "s5_credential_or_authority_change")]
    CredentialOrAuthorityChange,
    #[serde(rename = "side_effect_unknown")]
    Unknown,
}

/// Canonical V4 idempotency classes. Retry authority depends on the declared
/// class plus reconciliation evidence; the class alone never authorizes retry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionIdempotencyClass {
    #[serde(rename = "pure_read")]
    PureRead,
    #[serde(rename = "idempotent_write_with_key")]
    IdempotentWriteWithKey,
    #[serde(rename = "idempotent_by_observed_state")]
    IdempotentByObservedState,
    #[serde(rename = "compensatable_non_idempotent")]
    CompensatableNonIdempotent,
    #[serde(rename = "irreversible")]
    Irreversible,
    #[serde(rename = "idempotency_unknown")]
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
    MissingDecisionPrincipal,
    MissingActingPrincipal,
    MissingAuthorizationRevision,
    MissingPreconditionSnapshotCut,
    MissingExpectedPostcondition,
    InternalCaptureActionUnsupported,
}
