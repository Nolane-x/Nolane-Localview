use localview_live_bridge::{CanonicalQueuedAction, ConsequentialJournal, LiveBridge};
use localview_protocol::ProviderElementRef;
use localview_windows_uia_provider::{WindowsUiaDispatchContextRequirements, WindowsUiaPattern};
use thiserror::Error;

use crate::{
    WindowsObserveDispatchContextProvider, WindowsObserveRuntimeManager,
    WindowsUiaActionPreflightError, WindowsUiaActionPreflightRequest,
    WindowsUiaAuthorizationRevalidator, WindowsUiaDispatchExecutionArmError,
    WindowsUiaDispatchExecutor, WindowsUiaDispatchSealRequest, WindowsUiaPostconditionVerifier,
    WindowsUiaPreparedDispatchError, WindowsUiaPreparedDispatchRequest,
    WindowsUiaVerifiedExecutionError, WindowsUiaVerifiedExecutionOutcome,
    arm_uia_dispatch_execution, execute_armed_uia_dispatch_verified, prepare_uia_dispatch,
};

/// Exact provider target and volatile-context requirements for one already
/// canonicalized consequential Windows action.
///
/// Action id, session id, principal authority, risk/idempotency metadata, and
/// operation identity are deliberately absent. Those remain bound to the exact
/// `CanonicalQueuedAction` and its durable consequential records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVerifiedActionTarget {
    pub element_ref: ProviderElementRef,
    pub required_pattern: WindowsUiaPattern,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
}

#[derive(Debug, Error)]
pub enum WindowsUiaVerifiedActionCoordinatorError {
    #[error("Windows UIA canonical verified-action preflight failed: {0}")]
    Preflight(#[from] WindowsUiaActionPreflightError),
    #[error("Windows UIA canonical verified-action preparation failed: {0}")]
    Prepare(#[from] WindowsUiaPreparedDispatchError),
    #[error("Windows UIA canonical verified-action execution arm failed: {0}")]
    Arm(#[from] WindowsUiaDispatchExecutionArmError),
    #[error("Windows UIA canonical verified-action execution/verification failed: {0}")]
    Execute(#[from] WindowsUiaVerifiedExecutionError),
}

/// Close one exact canonical consequential Windows action through the existing
/// verified dispatch protocol.
///
/// This coordinator creates no canonical action, durable operation binding, or
/// recovery dispatch authority. It only composes the already-established gates:
/// semantic preflight, authority/context seal, durable PREPARED, second volatile
/// context arm, one-shot provider execution, fresh post-dispatch observation,
/// typed verification, reconciliation, and durable commit when proven.
///
/// The eight inputs are intentionally not collapsed into an authority-bearing
/// service bag: bridge state, durable journal, provider runtime, canonical
/// intent, exact target, independent authorization, executor, and verifier are
/// separate trust boundaries and remain explicit at this consequential call.
#[expect(
    clippy::too_many_arguments,
    reason = "keep eight consequential-action trust boundaries explicit instead of hiding them in a service bag"
)]
pub async fn execute_verified_canonical_uia_action<P, R, E, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    canonical: &CanonicalQueuedAction,
    target: WindowsUiaVerifiedActionTarget,
    revalidator: &R,
    executor: &E,
    verifier: &V,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaVerifiedActionCoordinatorError>
where
    P: WindowsObserveDispatchContextProvider,
    R: WindowsUiaAuthorizationRevalidator,
    E: WindowsUiaDispatchExecutor,
    V: WindowsUiaPostconditionVerifier,
{
    let session_id = canonical.action.session_id;
    let action_id = canonical.action.id;
    let authority = canonical.envelope.metadata.clone();

    let preflight = runtime
        .preflight_uia_action(
            session_id,
            WindowsUiaActionPreflightRequest {
                authority: authority.clone(),
                element_ref: target.element_ref,
                required_pattern: target.required_pattern,
            },
        )
        .await?;

    let prepared = prepare_uia_dispatch(
        bridge,
        journal,
        runtime,
        session_id,
        WindowsUiaPreparedDispatchRequest {
            seal: WindowsUiaDispatchSealRequest {
                action_id,
                authority,
                preflight,
                context_requirements: target.context_requirements,
            },
        },
        revalidator,
    )
    .await?;

    let armed = arm_uia_dispatch_execution(bridge, journal, runtime, session_id, prepared).await?;

    execute_armed_uia_dispatch_verified(
        bridge, journal, runtime, session_id, armed, executor, verifier,
    )
    .await
    .map_err(Into::into)
}
