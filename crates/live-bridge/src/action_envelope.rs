use chrono::Utc;
use localview_protocol::{
    ElementRef, PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{BridgeAction, BridgeActionKind, LiveBridge};

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

/// Payload-free operation identity for canonical actions.
///
/// This records only what operation class was authorized. Typed text, key
/// payloads, coordinates and other transport data are deliberately excluded
/// from the correctness/authority layer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalActionOperation {
    Activate,
    Select,
    Toggle,
    Expand,
    Collapse,
    SetValue,
    InputText,
    KeyInput,
    Scroll,
    Focus,
    Snapshot,
}

impl CanonicalActionOperation {
    pub fn from_bridge_action_kind(action: &BridgeActionKind) -> Option<Self> {
        match action {
            BridgeActionKind::Click => Some(Self::Activate),
            BridgeActionKind::TypeText { .. } => Some(Self::InputText),
            BridgeActionKind::Key { .. } => Some(Self::KeyInput),
            BridgeActionKind::Scroll { .. } => Some(Self::Scroll),
            BridgeActionKind::Focus => Some(Self::Focus),
            BridgeActionKind::Snapshot => Some(Self::Snapshot),
            BridgeActionKind::Measure
            | BridgeActionKind::FreezeVisuals
            | BridgeActionKind::RestoreVisuals { .. }
            | BridgeActionKind::CaptureScrollTo { .. }
            | BridgeActionKind::CaptureTileProbe { .. } => None,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundCanonicalDispatchError {
    MissingCanonicalEnvelope,
    EnvelopeMismatch,
    ActionIdentityMismatch,
    MissingProviderObservation,
    ProviderIncarnationMismatch,
    TargetIncarnationMismatch,
    InternalCaptureActionUnsupported,
    PublicQueueRejected,
}

impl LiveBridge {
    /// Bind a canonical V4.3 action for direct verified execution without ever
    /// exposing it to the legacy V1-V3 public action queue.
    ///
    /// This method mints no dispatch authority. It performs only the same
    /// admission-time envelope validation and current provider/target lineage
    /// binding as the queued canonical path. The caller must still durably admit
    /// the intent, bind its canonical operation, obtain independent current
    /// authorization, and execute through the verified-action coordinator.
    pub async fn bind_direct_canonical_action(
        &self,
        session_id: SessionId,
        reference: Option<ElementRef>,
        action: BridgeActionKind,
        metadata: ActionEnvelopeMetadata,
    ) -> Result<CanonicalQueuedAction, ActionEnvelopeBindingError> {
        if action.is_internal_capture_action() {
            return Err(ActionEnvelopeBindingError::InternalCaptureActionUnsupported);
        }
        if metadata.decision_principal_ref.as_str().trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingDecisionPrincipal);
        }
        if metadata.acting_principal_ref.as_str().trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingActingPrincipal);
        }
        if metadata.authorization_revision.trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingAuthorizationRevision);
        }
        if metadata.precondition_snapshot_cut_ref.trim().is_empty() {
            return Err(ActionEnvelopeBindingError::MissingPreconditionSnapshotCut);
        }
        if metadata.risk_class != ActionRiskClass::ObserveOnly
            && metadata.expected_postcondition_contract_refs.is_empty()
        {
            return Err(ActionEnvelopeBindingError::MissingExpectedPostcondition);
        }

        // Serialize direct binding against provider detach/release and all other
        // canonical action admissions. The legacy queue is intentionally never
        // touched while this gate is held.
        let _gate = self.action_gate.lock().await;
        let current_incarnations = {
            let continuity = self.continuity.read().await;
            let Some(state) = continuity.get(&session_id) else {
                return Err(ActionEnvelopeBindingError::MissingProviderObservation);
            };
            (
                state.provider_incarnation_ref.clone(),
                state.target_incarnation_ref.clone(),
            )
        };

        if metadata.provider_incarnation_ref != current_incarnations.0 {
            return Err(ActionEnvelopeBindingError::ProviderIncarnationMismatch);
        }
        if metadata.target_incarnation_ref != current_incarnations.1 {
            return Err(ActionEnvelopeBindingError::TargetIncarnationMismatch);
        }

        let action = BridgeAction {
            id: Uuid::new_v4(),
            session_id,
            reference,
            action,
            created_at: Utc::now(),
        };
        let envelope = CanonicalActionEnvelope {
            envelope_id: Uuid::new_v4(),
            transport_action_id: action.id,
            session_id,
            metadata,
        };
        self.action_envelopes
            .write()
            .await
            .insert(action.id, envelope.clone());

        Ok(CanonicalQueuedAction { action, envelope })
    }

    /// Discard one unconsumed direct canonical binding.
    ///
    /// This is used when a process-local confirmation expires or is otherwise
    /// invalidated before dispatch. Removal is serialized with provider/surface
    /// authority changes so an expired confirmation can never leave reusable
    /// canonical dispatch authority behind.
    pub async fn discard_bound_canonical_action(&self, action_id: Uuid) -> bool {
        let _gate = self.action_gate.lock().await;
        self.action_envelopes
            .write()
            .await
            .remove(&action_id)
            .is_some()
    }

    /// Consume one exact direct canonical binding into the public executor queue.
    ///
    /// The binding is consumed under the canonical action gate before the queue
    /// write, so provider detach, managed-surface authority rotation, and another
    /// canonical dispatch cannot race the lineage check. The stored envelope is
    /// removed even if queue admission fails: callers must reconcile/replan
    /// rather than retry a consequential dispatch after a failed one-shot handoff.
    pub async fn enqueue_bound_canonical_action_for_dispatch(
        &self,
        queued: CanonicalQueuedAction,
    ) -> Result<(), BoundCanonicalDispatchError> {
        if queued.action.action.is_internal_capture_action() {
            return Err(BoundCanonicalDispatchError::InternalCaptureActionUnsupported);
        }
        if queued.action.id != queued.envelope.transport_action_id
            || queued.action.session_id != queued.envelope.session_id
        {
            return Err(BoundCanonicalDispatchError::ActionIdentityMismatch);
        }

        let _gate = self.action_gate.lock().await;
        let stored = self
            .action_envelopes
            .read()
            .await
            .get(&queued.action.id)
            .cloned()
            .ok_or(BoundCanonicalDispatchError::MissingCanonicalEnvelope)?;
        if stored != queued.envelope {
            return Err(BoundCanonicalDispatchError::EnvelopeMismatch);
        }

        // From this point onward the dispatch attempt is one-shot, including
        // freshness rejection. Explicit confirmation must never become reusable
        // merely because the world changed before queue admission.
        self.action_envelopes.write().await.remove(&queued.action.id);

        let current_incarnations = {
            let continuity = self.continuity.read().await;
            let Some(state) = continuity.get(&queued.action.session_id) else {
                return Err(BoundCanonicalDispatchError::MissingProviderObservation);
            };
            (
                state.provider_incarnation_ref.clone(),
                state.target_incarnation_ref.clone(),
            )
        };
        if queued.envelope.metadata.provider_incarnation_ref != current_incarnations.0 {
            return Err(BoundCanonicalDispatchError::ProviderIncarnationMismatch);
        }
        if queued.envelope.metadata.target_incarnation_ref != current_incarnations.1 {
            return Err(BoundCanonicalDispatchError::TargetIncarnationMismatch);
        }

        if !self
            .legacy
            .enqueue_prebound_public_action(queued.action)
            .await
        {
            return Err(BoundCanonicalDispatchError::PublicQueueRejected);
        }
        Ok(())
    }
}
