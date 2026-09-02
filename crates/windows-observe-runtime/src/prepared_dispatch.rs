use localview_live_bridge::{
    ConsequentialJournal, ConsequentialJournalTransition, ConsequentialRecoveryState,
    DispatchPreparationReceipt, LiveBridge,
};
use localview_protocol::SessionId;
use thiserror::Error;
use uuid::Uuid;

use crate::{
    seal_uia_dispatch, WindowsObserveDispatchContextProvider, WindowsObserveRuntimeManager,
    WindowsUiaAuthorizationRevalidator, WindowsUiaDispatchSealError, WindowsUiaDispatchSealReceipt,
    WindowsUiaDispatchSealRequest,
};

/// Inputs for the final Windows data-only boundary before a future executor may
/// be considered. The caller supplies the existing seal request, not a seal
/// receipt, so it cannot manufacture provenance for durable PREPARED state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaPreparedDispatchRequest {
    pub seal: WindowsUiaDispatchSealRequest,
}

/// Opaque move-only capability proving that this process both completed the full
/// Windows dispatch seal and durably fsync'd the exact PREPARED journal record.
///
/// Deliberately not `Clone`: a future side-effect executor must consume one live
/// preparation capability by value. Reopening the journal cannot reconstruct
/// this type, matching the journal's non-durable continuation grant.
#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaPreparedDispatchReceipt {
    action_id: Uuid,
    seal: WindowsUiaDispatchSealReceipt,
    preparation: DispatchPreparationReceipt,
    preparation_journal_sequence: u64,
}

impl WindowsUiaPreparedDispatchReceipt {
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
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaPreparedDispatchError {
    #[error("Windows UIA dispatch seal failed before durable preparation: {0}")]
    Seal(#[from] WindowsUiaDispatchSealError),
    #[error("Windows UIA durable PREPARED append failed: {message}")]
    JournalWriteFailed { message: String },
    #[error("Windows UIA durable PREPARED journal entry does not match the exact sealed receipt")]
    PreparationEntryMismatch,
    #[error("Windows UIA canonical action envelope disappeared after durable PREPARED")]
    CanonicalEnvelopeMissingAfterPrepare,
    #[error("Windows UIA canonical action envelope changed during durable PREPARED")]
    CanonicalEnvelopeChangedAfterPrepare,
    #[error("Windows UIA canonical action is stale after durable PREPARED")]
    CanonicalEnvelopeStaleAfterPrepare,
    #[error("Windows UIA journal left DispatchPrepared before the prepared capability could be returned: {state:?}")]
    JournalStateChangedAfterPrepare {
        state: Option<ConsequentialRecoveryState>,
    },
}

/// Recompute the complete Windows dispatch seal and bind it to the generic V4.3
/// write-ahead PREPARED boundary.
///
/// Ordering is deliberately conservative:
/// 1. rerun semantic/live-element/canonical/principal/journal/provider-context seal;
/// 2. derive the PREPARED receipt only from the sealed authority;
/// 3. fsync PREPARED through `ConsequentialJournal`;
/// 4. verify the durable entry exactly matches the derived receipt;
/// 5. reread canonical freshness and journal state after the fsync.
///
/// If canonical authority changes while PREPARED is being written, this function
/// returns an error while the durable journal remains in `DispatchPrepared`, so
/// recovery is reconciliation-only. No UIA write pattern or OS input is emitted.
pub async fn prepare_uia_dispatch<P, R>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    request: WindowsUiaPreparedDispatchRequest,
    revalidator: &R,
) -> Result<WindowsUiaPreparedDispatchReceipt, WindowsUiaPreparedDispatchError>
where
    P: WindowsObserveDispatchContextProvider,
    R: WindowsUiaAuthorizationRevalidator,
{
    let action_id = request.seal.action_id;
    let sealed = seal_uia_dispatch(
        bridge,
        journal,
        runtime,
        session_id,
        request.seal,
        revalidator,
    )
    .await?;

    let metadata = &sealed.authority.authority;
    let preparation = DispatchPreparationReceipt {
        receipt_ref: format!("windows-uia:dispatch-prepared:{action_id}:{}", Uuid::new_v4()),
        authorization_journal_sequence: sealed.authority.authorization_journal_sequence,
        precondition_snapshot_cut_ref: metadata.precondition_snapshot_cut_ref.clone(),
        provider_incarnation_ref: metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: metadata.target_incarnation_ref.clone(),
    };

    let entry = journal
        .record_dispatch_prepared(action_id, preparation.clone())
        .await
        .map_err(|error| WindowsUiaPreparedDispatchError::JournalWriteFailed {
            message: error.to_string(),
        })?;

    match &entry.transition {
        ConsequentialJournalTransition::DispatchPrepared { receipt } if receipt == &preparation => {}
        _ => return Err(WindowsUiaPreparedDispatchError::PreparationEntryMismatch),
    }

    let envelope = bridge
        .action_envelope(action_id)
        .await
        .ok_or(WindowsUiaPreparedDispatchError::CanonicalEnvelopeMissingAfterPrepare)?;
    if envelope.session_id != session_id
        || envelope.transport_action_id != action_id
        || envelope.metadata != sealed.authority.authority
    {
        return Err(WindowsUiaPreparedDispatchError::CanonicalEnvelopeChangedAfterPrepare);
    }
    if !bridge.action_envelope_is_current(action_id).await {
        return Err(WindowsUiaPreparedDispatchError::CanonicalEnvelopeStaleAfterPrepare);
    }

    let state = journal.recovery_state(action_id).await;
    if state != Some(ConsequentialRecoveryState::DispatchPrepared) {
        return Err(WindowsUiaPreparedDispatchError::JournalStateChangedAfterPrepare { state });
    }

    Ok(WindowsUiaPreparedDispatchReceipt {
        action_id,
        seal: sealed,
        preparation,
        preparation_journal_sequence: entry.journal_sequence,
    })
}
