use localview_live_bridge::{
    ConsequentialJournal, ConsequentialRecoveryState, DispatchExecutionPermit,
    DispatchPreparationReceipt, LiveBridge,
};
use localview_protocol::SessionId;
use localview_windows_uia_provider::{
    evaluate_windows_uia_dispatch_context, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextBlocker, WindowsUiaDispatchContextRequest,
    WindowsUiaDispatchContextRequirements,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    WindowsObserveDispatchContextProvider, WindowsObserveRuntimeError,
    WindowsObserveRuntimeManager, WindowsUiaDispatchSealReceipt,
    WindowsUiaPreparedDispatchReceipt,
};

/// Opaque, move-only Windows execution authority created from one exact durable
/// PREPARED admission after a second volatile-context observation.
///
/// This is still data-only. It is deliberately not `Clone` and it does not
/// perform a UIA write pattern or emit keyboard/pointer input. A later executor
/// must consume the embedded generic permit and keep its final context recheck
/// adjacent to the actual provider side-effect.
#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaDispatchExecutionPermit {
    action_id: Uuid,
    seal: WindowsUiaDispatchSealReceipt,
    preparation: DispatchPreparationReceipt,
    preparation_journal_sequence: u64,
    armed_context: WindowsUiaBoundDispatchContextReceipt,
    pub(crate) dispatch_permit: DispatchExecutionPermit,
}

impl WindowsUiaDispatchExecutionPermit {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn seal(&self) -> &WindowsUiaDispatchSealReceipt {
        &self.seal
    }

    pub fn preparation(&self) -> &DispatchPreparationReceipt {
        &self.preparation
    }

    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }

    pub fn armed_context(&self) -> &WindowsUiaBoundDispatchContextReceipt {
        &self.armed_context
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchExecutionArmError {
    #[error("Windows UIA canonical action envelope disappeared before execution arm")]
    CanonicalEnvelopeMissingBeforeArm,
    #[error("Windows UIA canonical action envelope changed before execution arm")]
    CanonicalEnvelopeChangedBeforeArm,
    #[error("Windows UIA canonical action is stale before execution arm")]
    CanonicalEnvelopeStaleBeforeArm,
    #[error("Windows UIA journal is not durably PREPARED before execution arm: {state:?}")]
    JournalStateChangedBeforeArm {
        state: Option<ConsequentialRecoveryState>,
    },
    #[error("Windows UIA provider context revalidation failed during execution arm: {0}")]
    ContextProvider(#[from] WindowsObserveRuntimeError),
    #[error("Windows UIA execution-arm context receipt does not match the exact prepared authority/lease")]
    ContextReceiptMismatch,
    #[error("Windows UIA execution-arm volatile context is blocked: {0}")]
    ContextBlocked(#[from] WindowsUiaDispatchContextBlocker),
    #[error("Windows UIA canonical action envelope disappeared after execution-arm context observation")]
    CanonicalEnvelopeMissingAfterContext,
    #[error("Windows UIA canonical action envelope changed during execution-arm context observation")]
    CanonicalEnvelopeChangedAfterContext,
    #[error("Windows UIA canonical action is stale after execution-arm context observation")]
    CanonicalEnvelopeStaleAfterContext,
    #[error("Windows UIA journal left durable PREPARED during execution-arm context observation: {state:?}")]
    JournalStateChangedAfterContext {
        state: Option<ConsequentialRecoveryState>,
    },
    #[error("Windows UIA one-shot prepared capability could not begin dispatch: {message}")]
    BeginDispatchFailed { message: String },
    #[error("Windows UIA generic dispatch permit does not match the exact prepared action/sequence")]
    DispatchPermitMismatch,
}

/// Consume one Windows PREPARED receipt into a one-shot execution permit only
/// after the exact canonical authority, durable PREPARED state, retained element
/// lease, and volatile provider context are revalidated again.
///
/// Ordering is intentionally fail-closed:
/// 1. verify canonical identity/freshness and durable PREPARED state;
/// 2. re-observe foreground/focus/modal context against the exact retained lease
///    and the same requirement set sealed before PREPARED;
/// 3. independently evaluate the returned volatile observation instead of merely
///    trusting that a provider implementation did so;
/// 4. recheck canonical freshness and PREPARED state after the provider call;
/// 5. only then consume the generic prepared capability via `begin_dispatch`.
///
/// Any error consumes/drops the caller's Windows prepared receipt. Durable state
/// remains PREPARED, so the action is reconciliation-only rather than retryable.
/// This function performs no external side effect and is not the final
/// point-of-no-return for a future UIA write executor.
pub async fn arm_uia_dispatch_execution<P>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    prepared: WindowsUiaPreparedDispatchReceipt,
) -> Result<WindowsUiaDispatchExecutionPermit, WindowsUiaDispatchExecutionArmError>
where
    P: WindowsObserveDispatchContextProvider,
{
    let action_id = prepared.action_id();
    let seal = prepared.seal().clone();
    let preparation = prepared.preparation().clone();
    let preparation_journal_sequence = prepared.preparation_journal_sequence();

    verify_prepared_canonical_before_arm(bridge, journal, session_id, action_id, &seal).await?;

    let lease = &seal.authority.dispatch_revalidation.element_lease;
    let requirements = seal.context.requirements;
    let armed_context = runtime
        .revalidate_action_dispatch_context(
            session_id,
            WindowsUiaDispatchContextRequest {
                snapshot_cut_ref: lease.snapshot_cut_ref.clone(),
                element_ref: lease.element_ref.clone(),
                requirements,
            },
        )
        .await?;

    if !context_matches_prepared_lease(lease, requirements, &armed_context) {
        return Err(WindowsUiaDispatchExecutionArmError::ContextReceiptMismatch);
    }
    evaluate_windows_uia_dispatch_context(requirements, &armed_context.observation)?;

    verify_prepared_canonical_after_context(bridge, journal, session_id, action_id, &seal).await?;

    let dispatch_permit = journal
        .begin_dispatch(prepared.dispatch_capability)
        .await
        .map_err(|error| WindowsUiaDispatchExecutionArmError::BeginDispatchFailed {
            message: error.to_string(),
        })?;
    if dispatch_permit.action_id() != action_id
        || dispatch_permit.preparation_journal_sequence() != preparation_journal_sequence
    {
        return Err(WindowsUiaDispatchExecutionArmError::DispatchPermitMismatch);
    }

    Ok(WindowsUiaDispatchExecutionPermit {
        action_id,
        seal,
        preparation,
        preparation_journal_sequence,
        armed_context,
        dispatch_permit,
    })
}

async fn verify_prepared_canonical_before_arm(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    action_id: Uuid,
    seal: &WindowsUiaDispatchSealReceipt,
) -> Result<(), WindowsUiaDispatchExecutionArmError> {
    let envelope = bridge
        .action_envelope(action_id)
        .await
        .ok_or(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeMissingBeforeArm)?;
    if envelope.session_id != session_id
        || envelope.transport_action_id != action_id
        || envelope.metadata != seal.authority.authority
    {
        return Err(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeChangedBeforeArm);
    }
    if !bridge.action_envelope_is_current(action_id).await {
        return Err(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeStaleBeforeArm);
    }
    let state = journal.recovery_state(action_id).await;
    if state != Some(ConsequentialRecoveryState::DispatchPrepared) {
        return Err(WindowsUiaDispatchExecutionArmError::JournalStateChangedBeforeArm { state });
    }
    Ok(())
}

async fn verify_prepared_canonical_after_context(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    action_id: Uuid,
    seal: &WindowsUiaDispatchSealReceipt,
) -> Result<(), WindowsUiaDispatchExecutionArmError> {
    let envelope = bridge
        .action_envelope(action_id)
        .await
        .ok_or(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeMissingAfterContext)?;
    if envelope.session_id != session_id
        || envelope.transport_action_id != action_id
        || envelope.metadata != seal.authority.authority
    {
        return Err(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeChangedAfterContext);
    }
    if !bridge.action_envelope_is_current(action_id).await {
        return Err(WindowsUiaDispatchExecutionArmError::CanonicalEnvelopeStaleAfterContext);
    }
    let state = journal.recovery_state(action_id).await;
    if state != Some(ConsequentialRecoveryState::DispatchPrepared) {
        return Err(WindowsUiaDispatchExecutionArmError::JournalStateChangedAfterContext { state });
    }
    Ok(())
}

fn context_matches_prepared_lease(
    lease: &localview_windows_uia_provider::WindowsUiaElementLeaseReceipt,
    requirements: WindowsUiaDispatchContextRequirements,
    context: &WindowsUiaBoundDispatchContextReceipt,
) -> bool {
    context.requirements == requirements
        && context.snapshot_cut_ref == lease.snapshot_cut_ref
        && context.provider_incarnation_ref == lease.provider_incarnation_ref
        && context.target_incarnation_ref == lease.target_incarnation_ref
        && context.element_ref == lease.element_ref
}
