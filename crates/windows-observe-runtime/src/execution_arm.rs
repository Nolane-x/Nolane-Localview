use std::error::Error as StdError;

use localview_live_bridge::{
    ConsequentialJournal, ConsequentialJournalEntry, ConsequentialJournalTransition,
    ConsequentialRecoveryState, DispatchExecutionPermit, DispatchLinearizationReceipt,
    DispatchPreparationReceipt, LiveBridge,
};
use localview_protocol::{
    DispatchResult, ProviderElementRef, ProviderIncarnationRef, SessionId,
    TargetIncarnationRef, TransportResult,
};
use localview_windows_uia_provider::{
    evaluate_windows_uia_dispatch_context, WindowsUiaBoundDispatchContextReceipt,
    WindowsUiaDispatchContextBlocker, WindowsUiaDispatchContextRequest,
    WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
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

/// Opaque request presented to the one provider operation that is allowed to
/// become the final side-effect boundary in a later slice.
///
/// Its fields are private and it is deliberately not `Clone`: outside code
/// cannot manufacture or replay a provider request from action/cut identifiers.
/// The coordinator mints exactly one request from an already-sealed, durably
/// PREPARED, one-shot execution permit. A real Windows implementation must use
/// it only to locate the already-retained exact element and final-revalidate
/// target identity, volatile context and live pattern availability in the same
/// MTA command as the provider call.
#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaProviderExecutionRequest {
    dispatch_attempt_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    snapshot_cut_ref: String,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    element_ref: ProviderElementRef,
    required_pattern: WindowsUiaPattern,
    context_requirements: WindowsUiaDispatchContextRequirements,
}

impl WindowsUiaProviderExecutionRequest {
    pub fn dispatch_attempt_ref(&self) -> Uuid {
        self.dispatch_attempt_ref
    }

    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn preparation_journal_sequence(&self) -> u64 {
        self.preparation_journal_sequence
    }

    pub fn preparation_receipt_ref(&self) -> &str {
        &self.preparation_receipt_ref
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn element_ref(&self) -> &ProviderElementRef {
        &self.element_ref
    }

    pub fn required_pattern(&self) -> WindowsUiaPattern {
        self.required_pattern
    }

    pub fn context_requirements(&self) -> WindowsUiaDispatchContextRequirements {
        self.context_requirements
    }
}

/// Provider-owned evidence returned after one execution attempt.
///
/// Exact binding fields are repeated deliberately. The runtime accepts the
/// outcome only when every field exactly matches the opaque request it minted.
/// A mismatched receipt is treated as dispatch-uncertain and leaves durable state
/// PREPARED for reconciliation rather than guessing whether a side effect ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaProviderExecutionReceipt {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub transport_result: TransportResult,
    pub dispatch_result: DispatchResult,
}

/// Narrow provider execution surface. No implementation is supplied for the real
/// Windows UIA provider in this slice, so this trait does not enable writes.
///
/// The request is borrowed: the executor cannot obtain a replayable request value
/// from the coordinator. A future production implementation must execute one MTA
/// command that keeps final exact lease/context/live-pattern checks adjacent to
/// the UIA pattern call.
#[allow(async_fn_in_trait)]
pub trait WindowsUiaDispatchExecutor: Send + Sync {
    type Error: StdError + Send + Sync + 'static;

    async fn execute(
        &self,
        request: &WindowsUiaProviderExecutionRequest,
    ) -> Result<WindowsUiaProviderExecutionReceipt, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchExecutionResult {
    pub provider_receipt: WindowsUiaProviderExecutionReceipt,
    pub journal_entry: ConsequentialJournalEntry,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchExecutionCoordinatorError {
    #[error("Windows UIA canonical action envelope disappeared before provider execution")]
    CanonicalEnvelopeMissingBeforeExecutor,
    #[error("Windows UIA canonical action envelope changed before provider execution")]
    CanonicalEnvelopeChangedBeforeExecutor,
    #[error("Windows UIA canonical action is stale before provider execution")]
    CanonicalEnvelopeStaleBeforeExecutor,
    #[error("Windows UIA journal is not durably PREPARED before provider execution: {state:?}")]
    JournalStateChangedBeforeExecutor {
        state: Option<ConsequentialRecoveryState>,
    },
    #[error("Windows UIA provider execution attempt failed or became transport-uncertain: {message}")]
    ProviderExecutionFailed { message: String },
    #[error("Windows UIA provider execution receipt does not match the exact one-shot request")]
    ProviderReceiptMismatch,
    #[error("Windows UIA provider returned a receipt without executor delivery")]
    ProviderReceiptTransportMismatch,
    #[error("Windows UIA durable dispatch linearization append failed: {message}")]
    JournalLinearizationFailed { message: String },
    #[error("Windows UIA durable dispatch linearization entry did not match the exact provider outcome")]
    LinearizationEntryMismatch,
}

/// Consume an armed Windows permit through exactly one provider execution
/// attempt and durably record the returned dispatch outcome.
///
/// This coordinator intentionally does not implement a real OS side effect. It
/// establishes the authority and durability protocol that a later provider
/// implementation must obey:
/// 1. canonical authority and durable PREPARED are checked immediately before
///    handing control to the provider;
/// 2. the request is derived exclusively from the sealed exact lease, pattern,
///    lineage, context requirements and PREPARED record;
/// 3. a returned receipt must match that exact request and prove it reached the
///    executor;
/// 4. once a valid provider receipt exists, its outcome is fsync'd immediately
///    using the embedded one-shot generic dispatch permit.
///
/// There is deliberately no canonical-authority recheck between a valid provider
/// receipt and the durable append: the provider may already have caused a side
/// effect, so preserving actual dispatch evidence outranks a later authority
/// change. Provider error, forged receipt, or journal append failure consumes the
/// caller's one-shot permit and leaves PREPARED/reconciliation semantics rather
/// than creating blind retry authority.
pub async fn execute_armed_uia_dispatch<E>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    executor: &E,
) -> Result<WindowsUiaDispatchExecutionResult, WindowsUiaDispatchExecutionCoordinatorError>
where
    E: WindowsUiaDispatchExecutor,
{
    verify_armed_canonical_before_executor(bridge, journal, session_id, &armed).await?;

    let action_id = armed.action_id;
    let lease = &armed.seal.authority.dispatch_revalidation.element_lease;
    let request = WindowsUiaProviderExecutionRequest {
        dispatch_attempt_ref: Uuid::new_v4(),
        action_id,
        preparation_journal_sequence: armed.preparation_journal_sequence,
        preparation_receipt_ref: armed.preparation.receipt_ref.clone(),
        snapshot_cut_ref: lease.snapshot_cut_ref.clone(),
        provider_incarnation_ref: lease.provider_incarnation_ref.clone(),
        target_incarnation_ref: lease.target_incarnation_ref.clone(),
        element_ref: lease.element_ref.clone(),
        required_pattern: armed
            .seal
            .authority
            .dispatch_revalidation
            .preflight
            .required_pattern,
        context_requirements: armed.seal.context.requirements,
    };

    let provider_receipt = executor
        .execute(&request)
        .await
        .map_err(|error| WindowsUiaDispatchExecutionCoordinatorError::ProviderExecutionFailed {
            message: error.to_string(),
        })?;

    if !provider_receipt_matches_request(&provider_receipt, &request) {
        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptMismatch);
    }
    if provider_receipt.transport_result != TransportResult::DeliveredToExecutor {
        return Err(WindowsUiaDispatchExecutionCoordinatorError::ProviderReceiptTransportMismatch);
    }

    let linearization = DispatchLinearizationReceipt {
        receipt_ref: format!(
            "windows-uia:dispatch-attempt:{}",
            provider_receipt.dispatch_attempt_ref
        ),
        transport_result: provider_receipt.transport_result,
        dispatch_result: provider_receipt.dispatch_result,
    };
    let journal_entry = journal
        .record_dispatch_linearized(armed.dispatch_permit, linearization.clone())
        .await
        .map_err(|error| WindowsUiaDispatchExecutionCoordinatorError::JournalLinearizationFailed {
            message: error.to_string(),
        })?;

    if journal_entry.action_id != action_id
        || !matches!(
            &journal_entry.transition,
            ConsequentialJournalTransition::DispatchLinearized { receipt }
                if receipt == &linearization
        )
    {
        return Err(WindowsUiaDispatchExecutionCoordinatorError::LinearizationEntryMismatch);
    }

    Ok(WindowsUiaDispatchExecutionResult {
        provider_receipt,
        journal_entry,
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

async fn verify_armed_canonical_before_executor(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    armed: &WindowsUiaDispatchExecutionPermit,
) -> Result<(), WindowsUiaDispatchExecutionCoordinatorError> {
    let action_id = armed.action_id;
    let envelope = bridge
        .action_envelope(action_id)
        .await
        .ok_or(WindowsUiaDispatchExecutionCoordinatorError::CanonicalEnvelopeMissingBeforeExecutor)?;
    if envelope.session_id != session_id
        || envelope.transport_action_id != action_id
        || envelope.metadata != armed.seal.authority.authority
    {
        return Err(
            WindowsUiaDispatchExecutionCoordinatorError::CanonicalEnvelopeChangedBeforeExecutor,
        );
    }
    if !bridge.action_envelope_is_current(action_id).await {
        return Err(
            WindowsUiaDispatchExecutionCoordinatorError::CanonicalEnvelopeStaleBeforeExecutor,
        );
    }
    let state = journal.recovery_state(action_id).await;
    if state != Some(ConsequentialRecoveryState::DispatchPrepared) {
        return Err(
            WindowsUiaDispatchExecutionCoordinatorError::JournalStateChangedBeforeExecutor {
                state,
            },
        );
    }
    Ok(())
}

fn provider_receipt_matches_request(
    receipt: &WindowsUiaProviderExecutionReceipt,
    request: &WindowsUiaProviderExecutionRequest,
) -> bool {
    receipt.dispatch_attempt_ref == request.dispatch_attempt_ref
        && receipt.action_id == request.action_id
        && receipt.preparation_journal_sequence == request.preparation_journal_sequence
        && receipt.preparation_receipt_ref == request.preparation_receipt_ref
        && receipt.snapshot_cut_ref == request.snapshot_cut_ref
        && receipt.provider_incarnation_ref == request.provider_incarnation_ref
        && receipt.target_incarnation_ref == request.target_incarnation_ref
        && receipt.element_ref == request.element_ref
        && receipt.required_pattern == request.required_pattern
        && receipt.context_requirements == request.context_requirements
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
