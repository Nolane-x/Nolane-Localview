use localview_live_bridge::{
    ActionEnvelopeMetadata, ConsequentialJournal, ConsequentialRecoveryState, LiveBridge,
};
use localview_protocol::SessionId;
use localview_windows_uia_provider::{
    WindowsUiaBoundDispatchContextReceipt, WindowsUiaDispatchContextRequest,
    WindowsUiaDispatchContextRequirements,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    validate_uia_dispatch_authority, WindowsObserveDispatchContextProvider,
    WindowsObserveRuntimeError, WindowsObserveRuntimeManager, WindowsUiaActionPreflightReceipt,
    WindowsUiaAuthorizationRevalidator, WindowsUiaDispatchAuthorityError,
    WindowsUiaDispatchAuthorityReceipt, WindowsUiaDispatchRevalidationError,
    WindowsUiaDispatchRevalidationRequest,
};

/// Caller-owned inputs for the last data-only Windows dispatch seal.
///
/// The caller does not supply authority or provider-context receipts. Those are
/// recomputed inside `seal_uia_dispatch`, so a public receipt struct cannot be
/// used to manufacture provenance for a future side-effect boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchSealRequest {
    pub action_id: Uuid,
    pub authority: ActionEnvelopeMetadata,
    pub preflight: WindowsUiaActionPreflightReceipt,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
}

/// Sealed, data-only eligibility evidence. This proves that semantic evidence,
/// the exact retained live element, canonical principal/policy authority, the
/// durable journal lifecycle, and volatile provider context all agreed during
/// one bounded validation sequence.
///
/// It is still not a dispatch-linearization receipt and performs no UIA pattern
/// method or keyboard/pointer input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchSealReceipt {
    pub authority: WindowsUiaDispatchAuthorityReceipt,
    pub context: WindowsUiaBoundDispatchContextReceipt,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchSealError {
    #[error("Windows UIA dispatch semantic/live-element revalidation failed: {0}")]
    DispatchRevalidation(#[from] WindowsUiaDispatchRevalidationError),
    #[error("Windows UIA canonical/principal/journal authority validation failed: {0}")]
    Authority(#[from] WindowsUiaDispatchAuthorityError),
    #[error("Windows UIA provider dispatch-context observation failed: {0}")]
    ContextProvider(#[from] WindowsObserveRuntimeError),
    #[error("Windows UIA dispatch context receipt does not match the sealed authority/lease")]
    ContextReceiptMismatch,
    #[error("Windows UIA canonical action envelope disappeared during provider context validation")]
    CanonicalEnvelopeMissingAfterContext,
    #[error("Windows UIA canonical action envelope changed during provider context validation")]
    CanonicalEnvelopeChangedAfterContext,
    #[error("Windows UIA canonical action is stale after provider context validation")]
    CanonicalEnvelopeStaleAfterContext,
    #[error("Windows UIA durable journal left AuthorizedNotDispatched during provider context validation: {state:?}")]
    JournalStateChangedAfterContext {
        state: Option<ConsequentialRecoveryState>,
    },
}

/// Compose all currently-landed Phase 6 eligibility authorities into one
/// fail-closed seal without crossing a side-effect boundary.
///
/// Ordering is intentional:
/// 1. rerun semantic preflight and bind the exact retained worker element;
/// 2. bind that receipt to the canonical envelope, independent authorization,
///    and durable consequential journal;
/// 3. ask the provider for volatile foreground/focus/modal evidence against the
///    exact same retained element and snapshot cut;
/// 4. verify that provider receipt exactly matches the requested requirements
///    and the authority-bound lease;
/// 5. re-read canonical envelope freshness and durable journal state after the
///    provider call, closing the race where authority changes while context is
///    being observed.
///
/// A later side-effect executor must still linearize dispatch durably and
/// perform postcondition verification. This function never invokes UIA write
/// patterns and never sends OS input.
pub async fn seal_uia_dispatch<P, R>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    request: WindowsUiaDispatchSealRequest,
    revalidator: &R,
) -> Result<WindowsUiaDispatchSealReceipt, WindowsUiaDispatchSealError>
where
    P: WindowsObserveDispatchContextProvider,
    R: WindowsUiaAuthorizationRevalidator,
{
    let dispatch_revalidation = runtime
        .revalidate_uia_dispatch(
            session_id,
            WindowsUiaDispatchRevalidationRequest {
                authority: request.authority.clone(),
                preflight: request.preflight,
            },
        )
        .await?;

    let authority = validate_uia_dispatch_authority(
        bridge,
        journal,
        session_id,
        request.action_id,
        dispatch_revalidation,
        revalidator,
    )
    .await?;

    let lease = &authority.dispatch_revalidation.element_lease;
    let context = runtime
        .revalidate_action_dispatch_context(
            session_id,
            WindowsUiaDispatchContextRequest {
                snapshot_cut_ref: lease.snapshot_cut_ref.clone(),
                element_ref: lease.element_ref.clone(),
                requirements: request.context_requirements,
            },
        )
        .await?;

    if !context_matches_authority(&authority, request.context_requirements, &context) {
        return Err(WindowsUiaDispatchSealError::ContextReceiptMismatch);
    }

    let envelope = bridge
        .action_envelope(request.action_id)
        .await
        .ok_or(WindowsUiaDispatchSealError::CanonicalEnvelopeMissingAfterContext)?;
    if envelope.session_id != session_id
        || envelope.transport_action_id != request.action_id
        || envelope.metadata != authority.authority
    {
        return Err(WindowsUiaDispatchSealError::CanonicalEnvelopeChangedAfterContext);
    }
    if !bridge.action_envelope_is_current(request.action_id).await {
        return Err(WindowsUiaDispatchSealError::CanonicalEnvelopeStaleAfterContext);
    }

    let state = journal.recovery_state(request.action_id).await;
    if state != Some(ConsequentialRecoveryState::AuthorizedNotDispatched) {
        return Err(WindowsUiaDispatchSealError::JournalStateChangedAfterContext { state });
    }

    Ok(WindowsUiaDispatchSealReceipt { authority, context })
}

fn context_matches_authority(
    authority: &WindowsUiaDispatchAuthorityReceipt,
    requirements: WindowsUiaDispatchContextRequirements,
    context: &WindowsUiaBoundDispatchContextReceipt,
) -> bool {
    let lease = &authority.dispatch_revalidation.element_lease;
    context.requirements == requirements
        && context.snapshot_cut_ref == lease.snapshot_cut_ref
        && context.provider_incarnation_ref == lease.provider_incarnation_ref
        && context.target_incarnation_ref == lease.target_incarnation_ref
        && context.element_ref == lease.element_ref
}
