use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
};
use localview_protocol::{
    EventContinuityState, ProviderElementRealization, ProviderElementRef,
    ReconciliationCompleteness, SessionId,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaBooleanCapabilityFact, WindowsUiaElementLeaseReceipt,
    WindowsUiaElementLeaseRequest, WindowsUiaPattern, WindowsUiaPatternSupport,
    WindowsUiaValueCapabilityFacts,
};
use thiserror::Error;

use crate::runtime_manager::{
    WindowsObserveActionLeaseProvider, WindowsObserveProvider, WindowsObserveRuntimeError,
    WindowsObserveRuntimeManager, WindowsSemanticReadError, WindowsSemanticReadRequest,
};

/// Point-in-time capability/freshness request for a future Windows UIA action.
///
/// Passing this gate is necessary but never sufficient to dispatch an action.
/// Policy, risk, idempotency, principal authorization, journaling and immediate
/// dispatch-time revalidation remain separate authorities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaActionPreflightRequest {
    pub authority: ActionEnvelopeMetadata,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
}

/// Evidence that an exact immutable semantic node was current, realized and
/// advertised the required UIA pattern at one snapshot cut. This receipt does
/// not reserve the UI, lock provider state or authorize later dispatch.
///
/// The exact canonical action authority is retained so a later dispatch fence
/// can reject principal/policy/lineage substitution instead of accepting a
/// semantically equivalent but differently authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaActionPreflightReceipt {
    pub authority: ActionEnvelopeMetadata,
    pub snapshot_cut_ref: String,
    pub cache_revision_ref: String,
    pub observed_digest: String,
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaActionPreflightError {
    #[error("Windows UIA action preflight session {session_id} is not attached")]
    NotAttached { session_id: SessionId },
    #[error("Windows UIA action preflight canonical lineage authority rejected: {0:?}")]
    Authority(ActionEnvelopeBindingError),
    #[error("Windows UIA action preflight provider continuity is unreconciled: {continuity:?}")]
    ContinuityUnreconciled { continuity: EventContinuityState },
    #[error("Windows UIA observation authority changed while semantic preflight was reading")]
    ObservationChangedDuringPreflight,
    #[error("Windows UIA action preflight precondition cut does not match the current snapshot")]
    PreconditionSnapshotCutMismatch { expected: String, actual: String },
    #[error("Windows UIA action preflight current snapshot is incomplete")]
    SnapshotIncomplete,
    #[error(
        "Windows UIA action preflight element acquisition cut does not match the current snapshot"
    )]
    ElementAcquisitionCutMismatch { expected: String, actual: String },
    #[error("Windows UIA action preflight element does not exist in the exact current snapshot")]
    ElementNotFound,
    #[error("Windows UIA action preflight element is not currently realized: {realization:?}")]
    ElementNotRealized {
        realization: ProviderElementRealization,
    },
    #[error("Windows UIA action preflight pattern is unsupported: {pattern:?}")]
    PatternUnsupported { pattern: WindowsUiaPattern },
    #[error("Windows UIA action preflight pattern support is unknown: {pattern:?}")]
    PatternSupportUnknown { pattern: WindowsUiaPattern },
    #[error(
        "Windows UIA SetValue preflight requires explicit non-password writable facts; password={is_password:?}, read_only={is_read_only:?}"
    )]
    SetValueUnsafeCapability {
        is_password: WindowsUiaBooleanCapabilityFact,
        is_read_only: WindowsUiaBooleanCapabilityFact,
    },
    #[error("Windows UIA action preflight internal read gate invariant failed")]
    ReadGateInvariant,
}

/// Request presented immediately before a future Windows UIA side-effect
/// boundary. It carries both the current canonical authority and the exact
/// earlier preflight evidence that authority produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchRevalidationRequest {
    pub authority: ActionEnvelopeMetadata,
    pub preflight: WindowsUiaActionPreflightReceipt,
}

/// Data-only proof that dispatch-time semantic evidence remained current and
/// the worker could still bind the exact retained live UIA element. This is an
/// eligibility receipt only: no UIA pattern method or OS input is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchRevalidationReceipt {
    pub authority: ActionEnvelopeMetadata,
    pub preflight: WindowsUiaActionPreflightReceipt,
    pub element_lease: WindowsUiaElementLeaseReceipt,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaDispatchRevalidationError {
    #[error("Windows UIA dispatch authority does not exactly match preflight authority")]
    PreflightAuthorityMismatch,
    #[error("Windows UIA dispatch preflight evidence changed during immediate revalidation")]
    PreflightEvidenceMismatch,
    #[error("Windows UIA dispatch semantic revalidation failed: {0}")]
    Preflight(#[from] WindowsUiaActionPreflightError),
    #[error("Windows UIA dispatch exact element lease binding failed: {0}")]
    LeaseProvider(WindowsObserveRuntimeError),
    #[error("Windows UIA dispatch exact element lease receipt did not match revalidated authority")]
    LeaseReceiptMismatch,
}

impl<P> WindowsObserveRuntimeManager<P>
where
    P: WindowsObserveProvider,
{
    /// Validate point-in-time semantic capability without calling the provider.
    ///
    /// Existing continuity debt must already have an Established reconciliation
    /// receipt before cached semantic evidence is eligible for action planning.
    /// The observation status is sampled on both sides of the serialized semantic
    /// read and must remain exactly stable across that window. A gap/reset,
    /// reincarnation, reconciliation, detach, or even a newly accepted provider
    /// event therefore cannot race the cached read and be laundered into action
    /// evidence. The later dispatch revalidation repeats this gate again.
    pub async fn preflight_uia_action(
        &self,
        session_id: SessionId,
        request: WindowsUiaActionPreflightRequest,
    ) -> Result<WindowsUiaActionPreflightReceipt, WindowsUiaActionPreflightError> {
        let status_before = self
            .status(session_id)
            .await
            .ok_or(WindowsUiaActionPreflightError::NotAttached { session_id })?;
        require_reconciled_continuity(&status_before)?;

        let mut read_authority = request.authority.clone();
        read_authority.risk_class = ActionRiskClass::ObserveOnly;
        read_authority.idempotency_class = ActionIdempotencyClass::PureRead;

        let read = self
            .read_semantic(
                session_id,
                WindowsSemanticReadRequest {
                    authority: read_authority,
                    element_ref: request.element_ref,
                },
            )
            .await
            .map_err(map_read_error)?;

        let status_after = self
            .status(session_id)
            .await
            .ok_or(WindowsUiaActionPreflightError::NotAttached { session_id })?;
        if status_after != status_before {
            return Err(WindowsUiaActionPreflightError::ObservationChangedDuringPreflight);
        }
        require_reconciled_continuity(&status_after)?;

        let realization = read.node.element_ref.realization;
        if realization != ProviderElementRealization::RealizedCurrent {
            return Err(WindowsUiaActionPreflightError::ElementNotRealized { realization });
        }

        match WindowsUiaActionCapabilities::from_node(&read.node)
            .support_for(request.required_pattern)
        {
            WindowsUiaPatternSupport::Supported => {
                if request.required_pattern == WindowsUiaPattern::Value {
                    let facts = WindowsUiaValueCapabilityFacts::from_node(&read.node);
                    if !facts.permits_set_value() {
                        return Err(WindowsUiaActionPreflightError::SetValueUnsafeCapability {
                            is_password: facts.is_password(),
                            is_read_only: facts.is_read_only(),
                        });
                    }
                }
                Ok(WindowsUiaActionPreflightReceipt {
                    authority: request.authority,
                    snapshot_cut_ref: read.snapshot_cut_ref,
                    cache_revision_ref: read.cache_revision_ref,
                    observed_digest: read.observed_digest,
                    element_ref: read.node.element_ref,
                    required_pattern: request.required_pattern,
                })
            }
            WindowsUiaPatternSupport::Unsupported => {
                Err(WindowsUiaActionPreflightError::PatternUnsupported {
                    pattern: request.required_pattern,
                })
            }
            WindowsUiaPatternSupport::Unknown => {
                Err(WindowsUiaActionPreflightError::PatternSupportUnknown {
                    pattern: request.required_pattern,
                })
            }
        }
    }
}

impl<P> WindowsObserveRuntimeManager<P>
where
    P: WindowsObserveActionLeaseProvider,
{
    /// Revalidate the exact preflight authority/evidence immediately before a
    /// future dispatch boundary and bind the exact worker-owned live element.
    ///
    /// The semantic pass and worker lease bind intentionally use separate
    /// serialized operations. If observation changes between them, the worker's
    /// exact snapshot-cut lease check rejects the bind. No fuzzy re-resolution
    /// or side effect is permitted here.
    pub async fn revalidate_uia_dispatch(
        &self,
        session_id: SessionId,
        request: WindowsUiaDispatchRevalidationRequest,
    ) -> Result<WindowsUiaDispatchRevalidationReceipt, WindowsUiaDispatchRevalidationError> {
        if request.authority != request.preflight.authority {
            return Err(WindowsUiaDispatchRevalidationError::PreflightAuthorityMismatch);
        }

        let refreshed = self
            .preflight_uia_action(
                session_id,
                WindowsUiaActionPreflightRequest {
                    authority: request.authority.clone(),
                    element_ref: request.preflight.element_ref.clone(),
                    required_pattern: request.preflight.required_pattern,
                },
            )
            .await?;

        if refreshed != request.preflight {
            return Err(WindowsUiaDispatchRevalidationError::PreflightEvidenceMismatch);
        }

        let element_lease = self
            .bind_action_element_lease(
                session_id,
                WindowsUiaElementLeaseRequest {
                    snapshot_cut_ref: refreshed.snapshot_cut_ref.clone(),
                    element_ref: refreshed.element_ref.clone(),
                },
            )
            .await
            .map_err(WindowsUiaDispatchRevalidationError::LeaseProvider)?;

        if element_lease.snapshot_cut_ref != refreshed.snapshot_cut_ref
            || element_lease.provider_incarnation_ref != request.authority.provider_incarnation_ref
            || element_lease.target_incarnation_ref != request.authority.target_incarnation_ref
            || element_lease.element_ref != refreshed.element_ref
        {
            return Err(WindowsUiaDispatchRevalidationError::LeaseReceiptMismatch);
        }

        Ok(WindowsUiaDispatchRevalidationReceipt {
            authority: request.authority,
            preflight: refreshed,
            element_lease,
        })
    }
}

fn require_reconciled_continuity(
    status: &localview_live_bridge::ObservationStatus,
) -> Result<(), WindowsUiaActionPreflightError> {
    if continuity_requires_reconciliation(status.event_continuity)
        && status.current_snapshot_completeness != Some(ReconciliationCompleteness::Established)
    {
        return Err(WindowsUiaActionPreflightError::ContinuityUnreconciled {
            continuity: status.event_continuity,
        });
    }
    Ok(())
}

fn continuity_requires_reconciliation(continuity: EventContinuityState) -> bool {
    matches!(
        continuity,
        EventContinuityState::GapDetected
            | EventContinuityState::SequenceReset
            | EventContinuityState::ProviderReincarnated
            | EventContinuityState::ReconciliationRequired
            | EventContinuityState::ReconnectedUnreconciled
            | EventContinuityState::Broken
    )
}

fn map_read_error(error: WindowsSemanticReadError) -> WindowsUiaActionPreflightError {
    match error {
        WindowsSemanticReadError::NotAttached { session_id } => {
            WindowsUiaActionPreflightError::NotAttached { session_id }
        }
        WindowsSemanticReadError::Authority(error) => {
            WindowsUiaActionPreflightError::Authority(error)
        }
        WindowsSemanticReadError::PreconditionSnapshotCutMismatch { expected, actual } => {
            WindowsUiaActionPreflightError::PreconditionSnapshotCutMismatch { expected, actual }
        }
        WindowsSemanticReadError::SnapshotIncomplete => {
            WindowsUiaActionPreflightError::SnapshotIncomplete
        }
        WindowsSemanticReadError::ElementAcquisitionCutMismatch { expected, actual } => {
            WindowsUiaActionPreflightError::ElementAcquisitionCutMismatch { expected, actual }
        }
        WindowsSemanticReadError::ElementNotFound => {
            WindowsUiaActionPreflightError::ElementNotFound
        }
        WindowsSemanticReadError::ObserveOnlyRiskRequired
        | WindowsSemanticReadError::PureReadIdempotencyRequired => {
            WindowsUiaActionPreflightError::ReadGateInvariant
        }
    }
}
