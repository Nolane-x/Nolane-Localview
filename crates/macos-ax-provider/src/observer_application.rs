use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::application_incarnation::AxApplicationIncarnation;

static AX_OBSERVER_CREATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Provider-owned authority for one observer created for one exact application
/// incarnation. The token is not Clone/Copy so reincarnation validation consumes
/// the old observer authority instead of permitting accidental reuse.
#[derive(Debug, PartialEq, Eq)]
pub struct AxObserverApplicationBinding {
    application_incarnation: AxApplicationIncarnation,
    observer_creation_revision: u64,
}

impl AxObserverApplicationBinding {
    pub fn application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.application_incarnation
    }

    pub const fn observer_creation_revision(&self) -> u64 {
        self.observer_creation_revision
    }
}

/// Tombstone proving the observer lifecycle crossed an application-incarnation
/// boundary and therefore cannot retain continuity into the current process.
#[derive(Debug, PartialEq, Eq)]
pub struct AxReincarnatedObserverApplicationBinding {
    previous_application_incarnation: AxApplicationIncarnation,
    current_application_incarnation: AxApplicationIncarnation,
    invalidated_observer_creation_revision: u64,
}

impl AxReincarnatedObserverApplicationBinding {
    pub fn previous_application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.previous_application_incarnation
    }

    pub fn current_application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.current_application_incarnation
    }

    pub const fn invalidated_observer_creation_revision(&self) -> u64 {
        self.invalidated_observer_creation_revision
    }

    pub const fn recreate_directive(&self) -> AxObserverRecreateDirective {
        AxObserverRecreateDirective::CreateObserverForCurrentApplication
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxObserverRecreateDirective {
    CreateObserverForCurrentApplication,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AxObserverApplicationDecision {
    Current(AxObserverApplicationBinding),
    ApplicationReincarnated(AxReincarnatedObserverApplicationBinding),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxObserverApplicationRebindError {
    #[error("current application does not match the retired observer application lineage")]
    ApplicationLineageChanged,
    #[error("current application incarnation changed after observer reincarnation was detected")]
    CurrentIncarnationChanged,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AxObserverApplicationBindingProvider;

impl AxObserverApplicationBindingProvider {
    pub const fn new() -> Self {
        Self
    }

    pub fn bind_current(
        &self,
        application_incarnation: AxApplicationIncarnation,
    ) -> AxObserverApplicationBinding {
        AxObserverApplicationBinding {
            application_incarnation,
            observer_creation_revision: next_observer_creation_revision(),
        }
    }

    /// Consumes an observer binding and either preserves it for the exact same
    /// application incarnation or produces a tombstone requiring recreation.
    pub fn validate_current(
        &self,
        binding: AxObserverApplicationBinding,
        current_application_incarnation: &AxApplicationIncarnation,
    ) -> AxObserverApplicationDecision {
        if binding.application_incarnation == *current_application_incarnation {
            return AxObserverApplicationDecision::Current(binding);
        }

        AxObserverApplicationDecision::ApplicationReincarnated(
            AxReincarnatedObserverApplicationBinding {
                previous_application_incarnation: binding.application_incarnation,
                current_application_incarnation: current_application_incarnation.clone(),
                invalidated_observer_creation_revision: binding.observer_creation_revision,
            },
        )
    }

    /// Mints fresh observer authority only for the exact current incarnation of
    /// the same semantic application lineage that owned the retired observer.
    pub fn rebind_after_reincarnation(
        &self,
        retired: AxReincarnatedObserverApplicationBinding,
        current_application_incarnation: AxApplicationIncarnation,
    ) -> Result<AxObserverApplicationBinding, AxObserverApplicationRebindError> {
        if !retired
            .previous_application_incarnation
            .same_application_lineage(&current_application_incarnation)
        {
            return Err(AxObserverApplicationRebindError::ApplicationLineageChanged);
        }

        if retired.current_application_incarnation != current_application_incarnation {
            return Err(AxObserverApplicationRebindError::CurrentIncarnationChanged);
        }

        Ok(self.bind_current(current_application_incarnation))
    }
}

fn next_observer_creation_revision() -> u64 {
    AX_OBSERVER_CREATION_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}
