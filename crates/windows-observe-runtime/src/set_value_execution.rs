use std::{error::Error as StdError, fmt};

use localview_live_bridge::{
    CanonicalActionOperation, ConsequentialJournal, ConsequentialJournalTransition,
    ConsequentialPostconditionEvidence, ConsequentialPostconditionReconciliationReceipt,
    ConsequentialPostconditionStatus, ConsequentialRecoveryState, DispatchLinearizationReceipt,
    LiveBridge, SetValueMode, SetValuePayloadRef, reconcile_consequential_postconditions,
};
use localview_postcondition_contracts::{
    PayloadEqualityModeV1, PayloadEqualityPostconditionContractV1,
};
use localview_protocol::{
    DispatchResult, ProviderElementRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
    TransportResult, WorldOutcome,
};
use localview_windows_uia_provider::{
    WindowsUiaPattern, WindowsUiaSetValueDispatchReceipt, WindowsUiaSetValueDispatchRequest,
    WindowsUiaSetValueEquality, WindowsUiaSetValueVerificationReceipt,
    evaluate_windows_uia_dispatch_context,
};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    WindowsObserveProvider, WindowsObserveRuntimeManager, WindowsUiaDispatchExecutionPermit,
    WindowsUiaVerifiedExecutionOutcome,
};

/// Borrowed view of the already-confirmed, process-local SetValue capability.
///
/// The authoritative bytes stay owned by the caller for the entire dispatch +
/// verification transaction. The MTA dispatch receives one bounded temporary
/// zeroizing copy; this borrowed value remains available for the fresh equality
/// read and is never serialized into the generic execution path.
pub struct WindowsUiaSetValueExecutionPayload<'a> {
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    pub utf8_bytes: &'a [u8],
}

impl fmt::Debug for WindowsUiaSetValueExecutionPayload<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsUiaSetValueExecutionPayload")
            .field("payload_ref", &self.payload_ref)
            .field("mode", &self.mode)
            .field("utf8_len", &self.utf8_bytes.len())
            .finish()
    }
}

/// Fresh post-dispatch equality request presented only to the trusted provider
/// executor. The expected bytes are borrowed process-local authority and never
/// appear in Debug output or in the resulting verification receipt.
pub struct WindowsUiaSetValueVerificationRequest<'a> {
    action_id: Uuid,
    payload_ref: SetValuePayloadRef,
    mode: SetValueMode,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    element_ref: ProviderElementRef,
    observation_cut_ref: String,
    expected_utf8: &'a [u8],
}

impl fmt::Debug for WindowsUiaSetValueVerificationRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowsUiaSetValueVerificationRequest")
            .field("action_id", &self.action_id)
            .field("payload_ref", &self.payload_ref)
            .field("mode", &self.mode)
            .field("provider_incarnation_ref", &self.provider_incarnation_ref)
            .field("target_incarnation_ref", &self.target_incarnation_ref)
            .field("element_ref", &self.element_ref)
            .field("observation_cut_ref", &self.observation_cut_ref)
            .field("expected_utf8_len", &self.expected_utf8.len())
            .finish()
    }
}

impl<'a> WindowsUiaSetValueVerificationRequest<'a> {
    pub fn action_id(&self) -> Uuid {
        self.action_id
    }

    pub fn payload_ref(&self) -> SetValuePayloadRef {
        self.payload_ref
    }

    pub fn mode(&self) -> SetValueMode {
        self.mode
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

    pub fn observation_cut_ref(&self) -> &str {
        &self.observation_cut_ref
    }

    pub fn expected_utf8(&self) -> &'a [u8] {
        self.expected_utf8
    }
}

/// Narrow SetValue provider seam. Generic payload-free dispatch remains on
/// `WindowsUiaDispatchExecutor`; this trait is the only runtime surface that can
/// receive process-local SetValue bytes.
#[allow(async_fn_in_trait)]
pub trait WindowsUiaSetValueExecutor: Send + Sync {
    type Error: StdError + Send + Sync + 'static;

    async fn dispatch_set_value(
        &self,
        request: WindowsUiaSetValueDispatchRequest,
    ) -> Result<WindowsUiaSetValueDispatchReceipt, Self::Error>;

    async fn verify_set_value(
        &self,
        request: &WindowsUiaSetValueVerificationRequest<'_>,
    ) -> Result<WindowsUiaSetValueVerificationReceipt, Self::Error>;
}

#[derive(Debug, Error)]
pub enum WindowsUiaSetValueExecutionError {
    #[error("Windows UIA SetValue canonical action envelope is missing before executor")]
    CanonicalEnvelopeMissing,
    #[error("Windows UIA SetValue canonical action changed before executor")]
    CanonicalEnvelopeChanged,
    #[error("Windows UIA SetValue canonical action is stale before executor")]
    CanonicalEnvelopeStale,
    #[error("Windows UIA SetValue journal is not durably PREPARED before executor: {state:?}")]
    JournalStateChanged {
        state: Option<ConsequentialRecoveryState>,
    },
    #[error("Windows UIA SetValue canonical operation binding is missing or invalid")]
    CanonicalOperationMismatch,
    #[error("Windows UIA SetValue requires a sealed Value pattern")]
    RequiredPatternMismatch,
    #[error("Windows UIA SetValue durable payload binding is missing")]
    PayloadBindingMissing,
    #[error("Windows UIA SetValue process-local payload does not match durable binding metadata")]
    PayloadBindingMismatch,
    #[error("Windows UIA SetValue dispatch request rejected: {message}")]
    DispatchRequest { message: String },
    #[error("Windows UIA SetValue provider dispatch failed: {message}")]
    ProviderDispatch { message: String },
    #[error("Windows UIA SetValue provider receipt does not match the exact one-shot request")]
    ProviderReceiptMismatch,
    #[error("Windows UIA SetValue provider returned a receipt without executor delivery")]
    ProviderReceiptTransportMismatch,
    #[error("Windows UIA SetValue final volatile context receipt is invalid")]
    ProviderFinalContextMismatch,
    #[error("Windows UIA SetValue execution authority abandonment failed after {stage}: {message}")]
    ExecutionAuthorityAbandonmentFailed {
        stage: &'static str,
        message: String,
    },
    #[error("Windows UIA SetValue durable dispatch linearization failed: {message}")]
    JournalLinearizationFailed { message: String },
    #[error("Windows UIA SetValue dispatch linearization entry is inconsistent")]
    LinearizationEntryMismatch,
    #[error("Windows UIA SetValue post-dispatch observation authority failed: {message}")]
    ObservationAuthority { message: String },
    #[error("Windows UIA SetValue post-dispatch capture failed: {message}")]
    Capture { message: String },
    #[error("Windows UIA SetValue post-dispatch snapshot is not bound to the minted observation")]
    SnapshotBindingMismatch,
    #[error("Windows UIA SetValue admitted envelope is missing after dispatch")]
    AdmittedEnvelopeMissing,
    #[error("Windows UIA SetValue admitted envelope does not match fresh observation lineage")]
    AdmittedEnvelopeMismatch,
    #[error("Windows UIA SetValue expected postcondition is not the exact payload equality contract")]
    PayloadEqualityContractMismatch,
    #[error("Windows UIA SetValue fresh equality read failed: {message}")]
    Verification { message: String },
    #[error("Windows UIA SetValue equality receipt does not match the exact fresh request")]
    VerificationReceiptMismatch,
    #[error("Windows UIA SetValue postcondition reconciliation failed: {message}")]
    Reconciliation { message: String },
    #[error("Windows UIA SetValue durable commit failed: {message}")]
    Commit { message: String },
}

/// Consume one already-armed SetValue permit through exactly one semantic
/// mutation and one fresh equality observation.
///
/// Provider acknowledgement is never enough to commit. Any possibly-dispatched
/// outcome must obtain a journal-minted fresh observation cut and a provider-owned
/// equality receipt. `Mismatch` and `Unknown` remain non-committed; only `Match`
/// can reconcile to `VerifiedExpected` and cross the durable commit boundary.
pub async fn execute_armed_uia_set_value_dispatch<P, E>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    payload: WindowsUiaSetValueExecutionPayload<'_>,
    executor: &E,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaSetValueExecutionError>
where
    P: WindowsObserveProvider,
    E: WindowsUiaSetValueExecutor,
{
    let action_id = armed.action_id();
    let seal = armed.seal().clone();
    let preparation = armed.preparation().clone();
    let preparation_journal_sequence = armed.preparation_journal_sequence();
    let armed_context = armed.armed_context().clone();
    let mut dispatch_permit = Some(armed.dispatch_permit);

    let envelope = match bridge.action_envelope(action_id).await {
        Some(envelope) => envelope,
        None => {
            abandon(
                journal,
                dispatch_permit.take().expect("dispatch permit present"),
                "canonical_envelope_missing",
            )
            .await?;
            return Err(WindowsUiaSetValueExecutionError::CanonicalEnvelopeMissing);
        }
    };
    if envelope.session_id != session_id
        || envelope.transport_action_id != action_id
        || envelope.metadata != seal.authority.authority
    {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "canonical_envelope_changed",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::CanonicalEnvelopeChanged);
    }
    if !bridge.action_envelope_is_current(action_id).await {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "canonical_envelope_stale",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::CanonicalEnvelopeStale);
    }
    let state = journal.recovery_state(action_id).await;
    if state != Some(ConsequentialRecoveryState::DispatchPrepared) {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "journal_state_changed",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::JournalStateChanged { state });
    }

    let admitted_operation = journal
        .admitted_operation(action_id)
        .await
        .map_err(|_| WindowsUiaSetValueExecutionError::CanonicalOperationMismatch)?;
    if admitted_operation != Some(CanonicalActionOperation::SetValue) {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "canonical_operation_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::CanonicalOperationMismatch);
    }

    let required_pattern = seal.authority.dispatch_revalidation.preflight.required_pattern;
    if required_pattern != WindowsUiaPattern::Value {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "required_pattern_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::RequiredPatternMismatch);
    }

    let durable_payload = journal
        .set_value_payload_binding(action_id)
        .await
        .map_err(|_| WindowsUiaSetValueExecutionError::PayloadBindingMismatch)?
        .ok_or(WindowsUiaSetValueExecutionError::PayloadBindingMissing)?;
    let payload_len = u64::try_from(payload.utf8_bytes.len())
        .map_err(|_| WindowsUiaSetValueExecutionError::PayloadBindingMismatch)?;
    if durable_payload.action_id != action_id
        || durable_payload.payload_ref != payload.payload_ref
        || durable_payload.mode != payload.mode
        || durable_payload.payload_utf8_len != payload_len
    {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "payload_binding_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::PayloadBindingMismatch);
    }

    let lease = &seal.authority.dispatch_revalidation.element_lease;
    let dispatch_attempt_ref = Uuid::new_v4();
    let context_requirements = seal.context.requirements;
    let request = WindowsUiaSetValueDispatchRequest::new(
        dispatch_attempt_ref,
        action_id,
        preparation_journal_sequence,
        preparation.receipt_ref.clone(),
        lease.snapshot_cut_ref.clone(),
        lease.provider_incarnation_ref.clone(),
        lease.target_incarnation_ref.clone(),
        lease.element_ref.clone(),
        context_requirements,
        payload.payload_ref,
        payload.mode,
        payload.utf8_bytes.to_vec(),
    )
    .map_err(|error| WindowsUiaSetValueExecutionError::DispatchRequest {
        message: error.to_string(),
    })?;

    let receipt = match executor.dispatch_set_value(request).await {
        Ok(receipt) => receipt,
        Err(error) => {
            let message = error.to_string();
            abandon(
                journal,
                dispatch_permit.take().expect("dispatch permit present"),
                "provider_dispatch_failed",
            )
            .await?;
            return Err(WindowsUiaSetValueExecutionError::ProviderDispatch { message });
        }
    };

    if receipt.dispatch_attempt_ref != dispatch_attempt_ref
        || receipt.action_id != action_id
        || receipt.preparation_journal_sequence != preparation_journal_sequence
        || receipt.preparation_receipt_ref != preparation.receipt_ref
        || receipt.snapshot_cut_ref != lease.snapshot_cut_ref
        || receipt.provider_incarnation_ref != lease.provider_incarnation_ref
        || receipt.target_incarnation_ref != lease.target_incarnation_ref
        || receipt.element_ref != lease.element_ref
        || receipt.required_pattern != WindowsUiaPattern::Value
        || receipt.dispatch_operation != CanonicalActionOperation::SetValue
        || receipt.payload_ref != payload.payload_ref
        || receipt.mode != payload.mode
        || receipt.context_requirements != context_requirements
    {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "provider_receipt_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::ProviderReceiptMismatch);
    }
    if receipt.transport_result != TransportResult::DeliveredToExecutor {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "provider_transport_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::ProviderReceiptTransportMismatch);
    }
    if receipt.final_context.target_window_handle
        != armed_context.context.observation.target_window_handle
        || receipt.final_context.target_process_id
            != armed_context.context.observation.target_process_id
        || evaluate_windows_uia_dispatch_context(context_requirements, &receipt.final_context)
            .is_err()
    {
        abandon(
            journal,
            dispatch_permit.take().expect("dispatch permit present"),
            "provider_final_context_mismatch",
        )
        .await?;
        return Err(WindowsUiaSetValueExecutionError::ProviderFinalContextMismatch);
    }

    let linearization = DispatchLinearizationReceipt {
        receipt_ref: format!("windows-uia:set-value-dispatch-attempt:{dispatch_attempt_ref}"),
        transport_result: receipt.transport_result,
        dispatch_result: receipt.dispatch_result,
    };
    let journal_entry = journal
        .record_dispatch_linearized(
            dispatch_permit.take().expect("dispatch permit present"),
            linearization.clone(),
        )
        .await
        .map_err(|error| WindowsUiaSetValueExecutionError::JournalLinearizationFailed {
            message: error.to_string(),
        })?;
    if journal_entry.action_id != action_id
        || !matches!(
            &journal_entry.transition,
            ConsequentialJournalTransition::DispatchLinearized { receipt }
                if receipt == &linearization
        )
    {
        return Err(WindowsUiaSetValueExecutionError::LinearizationEntryMismatch);
    }

    let dispatch_journal_sequence = journal_entry.journal_sequence;
    let state = journal.recovery_state(action_id).await;
    if state == Some(ConsequentialRecoveryState::KnownNotDispatched) {
        return Ok(WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            action_id,
            dispatch_result: receipt.dispatch_result,
            dispatch_journal_sequence,
        });
    }
    if state != Some(ConsequentialRecoveryState::PossiblyDispatched) {
        return Err(WindowsUiaSetValueExecutionError::JournalStateChanged { state });
    }

    let observation_permit = journal
        .begin_postcondition_observation(action_id)
        .await
        .map_err(|error| WindowsUiaSetValueExecutionError::ObservationAuthority {
            message: error.to_string(),
        })?;
    let capture = runtime
        .capture_postcondition_observation_with_snapshot(journal, observation_permit)
        .await
        .map_err(|error| WindowsUiaSetValueExecutionError::Capture {
            message: error.to_string(),
        })?;
    let observation = capture.observation_receipt();
    let snapshot = capture.snapshot();
    if snapshot.snapshot_cut_ref() != observation.snapshot_cut_ref()
        || snapshot.provider_incarnation_ref() != observation.provider_incarnation_ref()
        || snapshot.target_incarnation_ref() != observation.target_incarnation_ref()
    {
        return Err(WindowsUiaSetValueExecutionError::SnapshotBindingMismatch);
    }

    let admitted_envelope = journal
        .entries_for(action_id)
        .await
        .into_iter()
        .find_map(|entry| match entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
            _ => None,
        })
        .ok_or(WindowsUiaSetValueExecutionError::AdmittedEnvelopeMissing)?;
    if admitted_envelope.transport_action_id != action_id
        || admitted_envelope.session_id != session_id
        || admitted_envelope.metadata.provider_incarnation_ref
            != *observation.provider_incarnation_ref()
        || admitted_envelope.metadata.target_incarnation_ref
            != *observation.target_incarnation_ref()
    {
        return Err(WindowsUiaSetValueExecutionError::AdmittedEnvelopeMismatch);
    }

    let contract_ref = exact_payload_equality_contract(
        &admitted_envelope.metadata.expected_postcondition_contract_refs,
        payload.payload_ref,
        payload.mode,
    )?;
    let verification_request = WindowsUiaSetValueVerificationRequest {
        action_id,
        payload_ref: payload.payload_ref,
        mode: payload.mode,
        provider_incarnation_ref: observation.provider_incarnation_ref().clone(),
        target_incarnation_ref: observation.target_incarnation_ref().clone(),
        element_ref: receipt.element_ref.clone(),
        observation_cut_ref: observation.snapshot_cut_ref().to_owned(),
        expected_utf8: payload.utf8_bytes,
    };
    let verification = executor
        .verify_set_value(&verification_request)
        .await
        .map_err(|error| WindowsUiaSetValueExecutionError::Verification {
            message: error.to_string(),
        })?;
    if verification.action_id != verification_request.action_id
        || verification.payload_ref != verification_request.payload_ref
        || verification.mode != verification_request.mode
        || verification.provider_incarnation_ref != verification_request.provider_incarnation_ref
        || verification.target_incarnation_ref != verification_request.target_incarnation_ref
        || verification.element_ref != verification_request.element_ref
        || verification.observation_cut_ref != verification_request.observation_cut_ref
    {
        return Err(WindowsUiaSetValueExecutionError::VerificationReceiptMismatch);
    }

    let status = match verification.equality {
        WindowsUiaSetValueEquality::Match => ConsequentialPostconditionStatus::VerifiedPass,
        WindowsUiaSetValueEquality::Mismatch => ConsequentialPostconditionStatus::VerifiedFail,
        WindowsUiaSetValueEquality::Unknown => ConsequentialPostconditionStatus::Unknown,
    };
    let evidence = ConsequentialPostconditionEvidence {
        contract_ref,
        status,
        receipt_ref: format!(
            "windows-uia:set-value-equality:{}:{}",
            action_id,
            verification.observation_cut_ref
        ),
    };
    let reconciliation = reconcile_consequential_postconditions(
        bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            capture.into_observation_receipt(),
            vec![evidence],
        ),
    )
    .await
    .map_err(|error| WindowsUiaSetValueExecutionError::Reconciliation {
        message: error.to_string(),
    })?;

    let reconciliation_journal_sequence = reconciliation.journal_entry.journal_sequence;
    if reconciliation.world_outcome == WorldOutcome::VerifiedExpected
        && reconciliation.postconditions_verified
    {
        let commit = journal.record_committed(action_id).await.map_err(|error| {
            WindowsUiaSetValueExecutionError::Commit {
                message: error.to_string(),
            }
        })?;
        return Ok(WindowsUiaVerifiedExecutionOutcome::Committed {
            action_id,
            world_outcome: reconciliation.world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
            commit_journal_sequence: commit.journal_sequence,
        });
    }

    Ok(WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
        action_id,
        world_outcome: reconciliation.world_outcome,
        dispatch_journal_sequence,
        reconciliation_journal_sequence,
    })
}

async fn abandon(
    journal: &ConsequentialJournal,
    permit: localview_live_bridge::DispatchExecutionPermit,
    stage: &'static str,
) -> Result<(), WindowsUiaSetValueExecutionError> {
    journal
        .abandon_dispatch_execution(permit)
        .await
        .map_err(
            |error| WindowsUiaSetValueExecutionError::ExecutionAuthorityAbandonmentFailed {
                stage,
                message: error.to_string(),
            },
        )
}

fn exact_payload_equality_contract(
    refs: &[String],
    payload_ref: SetValuePayloadRef,
    mode: SetValueMode,
) -> Result<String, WindowsUiaSetValueExecutionError> {
    if refs.len() != 1 {
        return Err(WindowsUiaSetValueExecutionError::PayloadEqualityContractMismatch);
    }
    let contract = PayloadEqualityPostconditionContractV1::from_contract_ref(&refs[0])
        .map_err(|_| WindowsUiaSetValueExecutionError::PayloadEqualityContractMismatch)?;
    let expected_mode = match mode {
        SetValueMode::ReplaceValue => PayloadEqualityModeV1::ReplaceValue,
        SetValueMode::ClearValue => PayloadEqualityModeV1::ClearValue,
    };
    if contract.mode != expected_mode || contract.payload_ref != payload_ref.0.to_string() {
        return Err(WindowsUiaSetValueExecutionError::PayloadEqualityContractMismatch);
    }
    Ok(refs[0].clone())
}
