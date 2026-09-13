use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::application_incarnation::AxApplicationIncarnation;

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;
const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
static AX_ELEMENT_BINDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Stable semantic coordinates used to reacquire a current AX element after
/// the provider reports that an earlier native handle can no longer safely
/// carry semantic-control authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxElementIdentity {
    application_incarnation: AxApplicationIncarnation,
    window_identity: String,
    semantic_identity: String,
}

impl AxElementIdentity {
    pub fn new(
        application_incarnation: AxApplicationIncarnation,
        window_identity: impl Into<String>,
        semantic_identity: impl Into<String>,
    ) -> Self {
        Self {
            application_incarnation,
            window_identity: window_identity.into(),
            semantic_identity: semantic_identity.into(),
        }
    }

    pub fn application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.application_incarnation
    }

    pub const fn application_pid(&self) -> i32 {
        self.application_incarnation.pid()
    }

    pub fn window_identity(&self) -> &str {
        &self.window_identity
    }

    pub fn semantic_identity(&self) -> &str {
        &self.semantic_identity
    }
}

/// Provider-owned capability binding for one live AX element incarnation.
///
/// This token is deliberately not `Clone`/`Copy`. An AX operation consumes it,
/// so neither an invalid-element result nor an inconclusive timeout can return
/// the same live binding for a blind retry.
#[derive(Debug, PartialEq, Eq)]
pub struct AxElementBinding {
    identity: AxElementIdentity,
    binding_sequence: u64,
}

impl AxElementBinding {
    pub const fn binding_sequence(&self) -> u64 {
        self.binding_sequence
    }

    pub fn identity(&self) -> &AxElementIdentity {
        &self.identity
    }
}

/// Tombstone proving which binding was invalidated because the native AX
/// element itself was stale.
#[derive(Debug, PartialEq, Eq)]
pub struct AxStaleElementBinding {
    identity: AxElementIdentity,
    invalidated_binding_sequence: u64,
}

impl AxStaleElementBinding {
    pub const fn invalidated_binding_sequence(&self) -> u64 {
        self.invalidated_binding_sequence
    }

    pub const fn reacquire_directive(&self) -> AxElementReacquireDirective {
        AxElementReacquireDirective::ReacquireCurrentIdentity
    }

    pub fn identity(&self) -> &AxElementIdentity {
        &self.identity
    }
}

/// Tombstone proving which binding encountered `kAXErrorCannotComplete`.
///
/// A cannot-complete result is deliberately not called stale: macOS documents
/// it as a messaging/unresponsive condition and the action may or may not have
/// reached the target. LocalView therefore consumes the old authority and
/// requires a current-identity reconciliation before any later attempt.
#[derive(Debug, PartialEq, Eq)]
pub struct AxUnresponsiveElementBinding {
    identity: AxElementIdentity,
    invalidated_binding_sequence: u64,
}

impl AxUnresponsiveElementBinding {
    pub const fn invalidated_binding_sequence(&self) -> u64 {
        self.invalidated_binding_sequence
    }

    pub const fn reacquire_directive(&self) -> AxElementReacquireDirective {
        AxElementReacquireDirective::ReacquireCurrentIdentity
    }

    pub fn identity(&self) -> &AxElementIdentity {
        &self.identity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxElementReacquireDirective {
    ReacquireCurrentIdentity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AxElementOperationDecision {
    Current(AxElementBinding),
    StaleTarget(AxStaleElementBinding),
    UnresponsiveTarget(AxUnresponsiveElementBinding),
    ProviderError { ax_error: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxElementRebindError {
    #[error("reacquired AX element does not match the invalidated semantic identity")]
    IdentityChanged,
}

/// M03/M04 authority boundary for AX element lifetime and inconclusive native
/// messaging outcomes.
#[derive(Debug, Clone, Copy, Default)]
pub struct AxElementBindingProvider;

impl AxElementBindingProvider {
    pub const fn new() -> Self {
        Self
    }

    pub fn bind_current(&self, identity: AxElementIdentity) -> AxElementBinding {
        AxElementBinding {
            identity,
            binding_sequence: next_binding_sequence(),
        }
    }

    /// Consumes the current binding together with the raw result of the AX
    /// operation performed through its native handle.
    ///
    /// `kAXErrorInvalidUIElement` never returns a live binding because the
    /// handle is stale. `kAXErrorCannotComplete` also never returns a live
    /// binding because delivery/effect is inconclusive; blindly repeating an
    /// action could duplicate a consequential world effect.
    pub fn observe_operation(
        &self,
        binding: AxElementBinding,
        ax_error: i32,
    ) -> AxElementOperationDecision {
        match ax_error {
            AX_ERROR_SUCCESS => AxElementOperationDecision::Current(binding),
            AX_ERROR_INVALID_UI_ELEMENT => {
                AxElementOperationDecision::StaleTarget(AxStaleElementBinding {
                    identity: binding.identity,
                    invalidated_binding_sequence: binding.binding_sequence,
                })
            }
            AX_ERROR_CANNOT_COMPLETE => {
                AxElementOperationDecision::UnresponsiveTarget(AxUnresponsiveElementBinding {
                    identity: binding.identity,
                    invalidated_binding_sequence: binding.binding_sequence,
                })
            }
            other => AxElementOperationDecision::ProviderError { ax_error: other },
        }
    }

    /// Mints a new binding only after the caller has reacquired an AX element
    /// matching the same application/window/semantic identity after a stale
    /// native handle.
    pub fn rebind_after_reacquire(
        &self,
        stale: AxStaleElementBinding,
        current_identity: AxElementIdentity,
    ) -> Result<AxElementBinding, AxElementRebindError> {
        rebind_same_identity(stale.identity, current_identity)
    }

    /// Mints a new binding only after an unresponsive/cannot-complete outcome
    /// has been reconciled by resolving the current target and proving that its
    /// application/window/semantic identity is unchanged.
    ///
    /// This intentionally requires a different consumed tombstone type from
    /// M03 so callers cannot mistake a messaging timeout for proof that the
    /// native element was stale.
    pub fn rebind_after_unresponsive_reacquire(
        &self,
        unresponsive: AxUnresponsiveElementBinding,
        current_identity: AxElementIdentity,
    ) -> Result<AxElementBinding, AxElementRebindError> {
        rebind_same_identity(unresponsive.identity, current_identity)
    }
}

fn rebind_same_identity(
    invalidated_identity: AxElementIdentity,
    current_identity: AxElementIdentity,
) -> Result<AxElementBinding, AxElementRebindError> {
    if invalidated_identity != current_identity {
        return Err(AxElementRebindError::IdentityChanged);
    }

    Ok(AxElementBinding {
        identity: current_identity,
        binding_sequence: next_binding_sequence(),
    })
}

fn next_binding_sequence() -> u64 {
    AX_ELEMENT_BINDING_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_provider_error_consumes_binding_without_claiming_staleness_or_timeout() {
        let provider = AxElementBindingProvider::new();
        let app = AxApplicationIncarnation::new("test.application", 7, 1);
        let binding = provider.bind_current(AxElementIdentity::new(app, "window", "target"));

        assert_eq!(
            provider.observe_operation(binding, -25205),
            AxElementOperationDecision::ProviderError { ax_error: -25205 }
        );
    }
}
