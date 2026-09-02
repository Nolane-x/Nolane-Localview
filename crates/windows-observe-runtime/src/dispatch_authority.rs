use std::error::Error as StdError;

use localview_live_bridge::{
    ActionEnvelopeMetadata, ConsequentialJournal, ConsequentialJournalTransition,
    ConsequentialRecoveryState, LiveBridge,
};
use localview_protocol::{PrincipalRef, SessionId};
use thiserror::Error;
use uuid::Uuid;

use crate::WindowsUiaDispatchRevalidationReceipt;

/// Evidence returned by the authority source that owns the current policy /
/// principal decision. The Windows runtime never manufactures this receipt from
/// an old `authorization_revision`; a caller must provide a real revalidator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAuthorizationRevalidationReceipt {
    pub action_id: Uuid,
    pub decision_principal_ref: PrincipalRef,
    pub acting_principal_ref: PrincipalRef,
    pub authorization_revision: String,
}

/// Narrow authority seam used at the last non-side-effect dispatch boundary.
///
/// Implementations may consult a policy engine, approval authority or another
/// canonical authorization source, but must return a receipt bound to the exact
/// action and principals. Merely echoing the previous revision is not sufficient
/// unless that implementation itself is the authority for the revision.
pub trait WindowsUiaAuthorizationRevalidator: Send + Sync {
    type Error: StdError + Send + Sync + 'static;

    fn revalidate(
        &self,
        action_id: Uuid,
        authority: &ActionEnvelopeMetadata,
    ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error>;
}

/// Eligibility evidence produced only after the canonical envelope, durable
/// journal lifecycle, independent authorization source and exact UIA semantic /
/// live-element revalidation all agree.
///
/// This receipt still does not execute a UIA pattern or OS input operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchAuthorityReceipt {
    pub action_id: Uuid,
    pub authority: ActionEnvelopeMetadata,
    pub dispatch_revalidation: WindowsUiaDispatchRevalidationReceipt,
    pub authorization_journal_sequence: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchAuthorityError {
    #[error("Windows UIA dispatch canonical action envelope is missing")]
    CanonicalEnvelopeMissing,
    #[error("Windows UIA dispatch canonical action belongs to a different session")]
    SessionMismatch,
    #[error("Windows UIA dispatch canonical authority does not match the revalidated authority")]
    CanonicalAuthorityMismatch,
    #[error("Windows UIA dispatch canonical provider/target incarnation is no longer current")]
    CanonicalEnvelopeStale,
    #[error("Windows UIA dispatch revalidation receipt is internally inconsistent")]
    DispatchRevalidationReceiptMismatch,
    #[error("Windows UIA dispatch durable intent admission is missing")]
    JournalIntentMissing,
    #[error("Windows UIA dispatch durable intent envelope differs from canonical authority")]
    JournalEnvelopeMismatch,
    #[error("Windows UIA dispatch journal state cannot authorize a new dispatch: {state:?}")]
    JournalStateNotDispatchable {
        state: Option<ConsequentialRecoveryState>,
    },
    #[error("Windows UIA authorization revalidation failed: {message}")]
    AuthorizationRevalidationFailed { message: String },
    #[error("Windows UIA authorization revalidation receipt does not match canonical authority")]
    AuthorizationReceiptMismatch,
    #[error("Windows UIA durable authorization revalidation append failed: {message}")]
    JournalWriteFailed { message: String },
}

/// Bind the exact current Windows UIA dispatch eligibility evidence to the
/// canonical V4.3 action and durable consequential lifecycle.
///
/// Ordering is deliberate:
/// 1. prove the exact canonical envelope is still current;
/// 2. prove the supplied semantic/live-element receipt is self-consistent;
/// 3. prove the journal has the exact admitted envelope and has not crossed a
///    dispatch/world-outcome boundary;
/// 4. ask the independent authorization authority to revalidate principals and
///    authorization revision;
/// 5. only then durably record `revalidated = true` and return eligibility.
///
/// No provider side effect is performed here. A later provider-context fence
/// must still revalidate foreground/focus/modal state immediately before any
/// actual UIA write or OS input linearization point.
pub async fn validate_uia_dispatch_authority<R>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    action_id: Uuid,
    dispatch_revalidation: WindowsUiaDispatchRevalidationReceipt,
    revalidator: &R,
) -> Result<WindowsUiaDispatchAuthorityReceipt, WindowsUiaDispatchAuthorityError>
where
    R: WindowsUiaAuthorizationRevalidator,
{
    let envelope = bridge
        .action_envelope(action_id)
        .await
        .ok_or(WindowsUiaDispatchAuthorityError::CanonicalEnvelopeMissing)?;

    if envelope.session_id != session_id || envelope.transport_action_id != action_id {
        return Err(WindowsUiaDispatchAuthorityError::SessionMismatch);
    }
    if envelope.metadata != dispatch_revalidation.authority {
        return Err(WindowsUiaDispatchAuthorityError::CanonicalAuthorityMismatch);
    }
    if !bridge.action_envelope_is_current(action_id).await {
        return Err(WindowsUiaDispatchAuthorityError::CanonicalEnvelopeStale);
    }
    if !dispatch_revalidation_is_self_consistent(&dispatch_revalidation) {
        return Err(WindowsUiaDispatchAuthorityError::DispatchRevalidationReceiptMismatch);
    }

    let entries = journal.entries_for(action_id).await;
    let admitted_envelope = entries.iter().find_map(|entry| match &entry.transition {
        ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
        _ => None,
    });
    let admitted_envelope = admitted_envelope
        .ok_or(WindowsUiaDispatchAuthorityError::JournalIntentMissing)?;
    if admitted_envelope != &envelope {
        return Err(WindowsUiaDispatchAuthorityError::JournalEnvelopeMismatch);
    }

    let state = journal.recovery_state(action_id).await;
    if !matches!(
        state,
        Some(ConsequentialRecoveryState::Admitted)
            | Some(ConsequentialRecoveryState::AuthorizedNotDispatched)
    ) {
        return Err(WindowsUiaDispatchAuthorityError::JournalStateNotDispatchable { state });
    }

    let authorization = revalidator
        .revalidate(action_id, &envelope.metadata)
        .map_err(|error| WindowsUiaDispatchAuthorityError::AuthorizationRevalidationFailed {
            message: error.to_string(),
        })?;
    if authorization.action_id != action_id
        || authorization.decision_principal_ref != envelope.metadata.decision_principal_ref
        || authorization.acting_principal_ref != envelope.metadata.acting_principal_ref
        || authorization.authorization_revision != envelope.metadata.authorization_revision
    {
        return Err(WindowsUiaDispatchAuthorityError::AuthorizationReceiptMismatch);
    }

    let authorization_entry = journal
        .record_authorization(
            action_id,
            authorization.authorization_revision,
            true,
        )
        .await
        .map_err(|error| WindowsUiaDispatchAuthorityError::JournalWriteFailed {
            message: error.to_string(),
        })?;

    Ok(WindowsUiaDispatchAuthorityReceipt {
        action_id,
        authority: envelope.metadata,
        dispatch_revalidation,
        authorization_journal_sequence: authorization_entry.journal_sequence,
    })
}

fn dispatch_revalidation_is_self_consistent(
    receipt: &WindowsUiaDispatchRevalidationReceipt,
) -> bool {
    let authority = &receipt.authority;
    let preflight = &receipt.preflight;
    let lease = &receipt.element_lease;

    authority == &preflight.authority
        && authority.precondition_snapshot_cut_ref == preflight.snapshot_cut_ref
        && preflight.snapshot_cut_ref == lease.snapshot_cut_ref
        && authority.provider_incarnation_ref == lease.provider_incarnation_ref
        && authority.target_incarnation_ref == lease.target_incarnation_ref
        && preflight.element_ref == lease.element_ref
        && preflight.element_ref.provider_incarnation_ref == authority.provider_incarnation_ref
        && preflight.element_ref.target_incarnation_ref == authority.target_incarnation_ref
        && preflight.element_ref.acquisition_cut_ref == preflight.snapshot_cut_ref
}
