use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;
static AX_ELEMENT_BINDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Stable semantic coordinates used to reacquire a current AX element after
/// the provider reports that an earlier `AXUIElementRef` is invalid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxElementIdentity {
    application_pid: i32,
    window_identity: String,
    semantic_identity: String,
}

impl AxElementIdentity {
    pub fn new(
        application_pid: i32,
        window_identity: impl Into<String>,
        semantic_identity: impl Into<String>,
    ) -> Self {
        Self {
            application_pid,
            window_identity: window_identity.into(),
            semantic_identity: semantic_identity.into(),
        }
    }

    pub const fn application_pid(&self) -> i32 {
        self.application_pid
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
/// so an invalid-element result cannot return the same live binding for retry.
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

/// Tombstone proving which binding was invalidated by the provider.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxElementReacquireDirective {
    ReacquireCurrentIdentity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AxElementOperationDecision {
    Current(AxElementBinding),
    StaleTarget(AxStaleElementBinding),
    ProviderError { ax_error: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxElementRebindError {
    #[error("reacquired AX element does not match the invalidated semantic identity")]
    IdentityChanged,
}

/// M03 authority boundary for AX element lifetime.
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
    /// `kAXErrorInvalidUIElement` never returns a live binding. Recovery must
    /// resolve a new native AX object from the tombstone's semantic identity.
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
            other => AxElementOperationDecision::ProviderError { ax_error: other },
        }
    }

    /// Mints a new binding only after the caller has reacquired an AX element
    /// matching the same application/window/semantic identity.
    pub fn rebind_after_reacquire(
        &self,
        stale: AxStaleElementBinding,
        current_identity: AxElementIdentity,
    ) -> Result<AxElementBinding, AxElementRebindError> {
        if stale.identity != current_identity {
            return Err(AxElementRebindError::IdentityChanged);
        }

        Ok(AxElementBinding {
            identity: current_identity,
            binding_sequence: next_binding_sequence(),
        })
    }
}

fn next_binding_sequence() -> u64 {
    AX_ELEMENT_BINDING_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_provider_error_consumes_binding_without_claiming_staleness() {
        let provider = AxElementBindingProvider::new();
        let binding = provider.bind_current(AxElementIdentity::new(7, "window", "target"));

        assert_eq!(
            provider.observe_operation(binding, -25204),
            AxElementOperationDecision::ProviderError { ax_error: -25204 }
        );
    }
}
