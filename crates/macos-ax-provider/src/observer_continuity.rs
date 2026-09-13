use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::{AxApplicationIncarnation, AxObserverApplicationBinding};

static AX_RUN_LOOP_SOURCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AX_OBSERVER_CONTINUITY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Provider-owned identity for one concrete AX observer CFRunLoop source
/// lifetime. A new source gets a new incarnation even when attached to the
/// same observer/application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AxRunLoopSourceIncarnation(u64);

impl AxRunLoopSourceIncarnation {
    pub const fn sequence(self) -> u64 {
        self.0
    }
}

/// Evidence about whether the observer run-loop source is currently being
/// serviced. Registration success alone is deliberately not a liveness proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxRunLoopLiveness {
    Serviced,
    Interrupted,
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxObserverContinuityStartError {
    #[error("observer run-loop source is not proven actively serviced")]
    RunLoopNotServiced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxObserverContinuityBreakReason {
    RunLoopInterrupted,
    RunLoopLivenessUnverified,
    RunLoopSourceReplaced,
}

/// Live event-continuity authority. This token is intentionally move-only:
/// validating it consumes the old authority so a broken lineage cannot be
/// accidentally reused after run-loop interruption/source replacement.
#[derive(Debug, PartialEq, Eq)]
pub struct AxObserverContinuityBinding {
    observer_binding: AxObserverApplicationBinding,
    run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    continuity_revision: u64,
}

impl AxObserverContinuityBinding {
    pub fn application_incarnation(&self) -> &AxApplicationIncarnation {
        self.observer_binding.application_incarnation()
    }

    pub const fn observer_creation_revision(&self) -> u64 {
        self.observer_binding.observer_creation_revision()
    }

    pub const fn run_loop_source_incarnation(&self) -> AxRunLoopSourceIncarnation {
        self.run_loop_source_incarnation
    }

    pub const fn continuity_revision(&self) -> u64 {
        self.continuity_revision
    }
}

/// Tombstone for one broken observer/run-loop continuity lineage.
#[derive(Debug, PartialEq, Eq)]
pub struct AxBrokenObserverContinuity {
    application_incarnation: AxApplicationIncarnation,
    previous_run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    current_run_loop_source_incarnation: Option<AxRunLoopSourceIncarnation>,
    invalidated_observer_creation_revision: u64,
    invalidated_continuity_revision: u64,
    reason: AxObserverContinuityBreakReason,
}

impl AxBrokenObserverContinuity {
    pub fn application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.application_incarnation
    }

    pub const fn previous_run_loop_source_incarnation(&self) -> AxRunLoopSourceIncarnation {
        self.previous_run_loop_source_incarnation
    }

    pub const fn current_run_loop_source_incarnation(
        &self,
    ) -> Option<AxRunLoopSourceIncarnation> {
        self.current_run_loop_source_incarnation
    }

    pub const fn invalidated_observer_creation_revision(&self) -> u64 {
        self.invalidated_observer_creation_revision
    }

    pub const fn invalidated_continuity_revision(&self) -> u64 {
        self.invalidated_continuity_revision
    }

    pub const fn reason(&self) -> AxObserverContinuityBreakReason {
        self.reason
    }

    pub const fn requires_snapshot_reconciliation(&self) -> bool {
        true
    }

    pub const fn requires_observer_recreation(&self) -> bool {
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AxObserverContinuityDecision {
    Current(AxObserverContinuityBinding),
    Broken(AxBrokenObserverContinuity),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxObserverContinuityRebindError {
    #[error("fresh observer belongs to a different application incarnation")]
    ApplicationIncarnationChanged,
    #[error("fresh observer creation revision is not newer than the broken lineage")]
    ObserverRevisionNotNewer,
    #[error("recovery attempted to reuse the invalidated run-loop source incarnation")]
    RunLoopSourceNotReplaced,
    #[error("replacement observer run-loop source is not proven actively serviced")]
    RunLoopNotServiced,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AxObserverContinuityProvider;

impl AxObserverContinuityProvider {
    pub const fn new() -> Self {
        Self
    }

    pub fn new_run_loop_source_incarnation(&self) -> AxRunLoopSourceIncarnation {
        AxRunLoopSourceIncarnation(
            AX_RUN_LOOP_SOURCE_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1,
        )
    }

    /// Establishes continuity only after the run-loop source is proven actively
    /// serviced. Observer construction/notification registration are separate
    /// evidence and cannot mint continuity by themselves.
    pub fn activate(
        &self,
        observer_binding: AxObserverApplicationBinding,
        run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        liveness: AxRunLoopLiveness,
    ) -> Result<AxObserverContinuityBinding, AxObserverContinuityStartError> {
        if liveness != AxRunLoopLiveness::Serviced {
            return Err(AxObserverContinuityStartError::RunLoopNotServiced);
        }

        Ok(AxObserverContinuityBinding {
            observer_binding,
            run_loop_source_incarnation,
            continuity_revision: next_continuity_revision(),
        })
    }

    /// Consumes live continuity and either preserves the exact same serviced
    /// run-loop source lineage or converts it to an irreversible tombstone.
    pub fn validate(
        &self,
        binding: AxObserverContinuityBinding,
        current_run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        liveness: AxRunLoopLiveness,
    ) -> AxObserverContinuityDecision {
        if current_run_loop_source_incarnation != binding.run_loop_source_incarnation {
            return AxObserverContinuityDecision::Broken(broken_from(
                binding,
                Some(current_run_loop_source_incarnation),
                AxObserverContinuityBreakReason::RunLoopSourceReplaced,
            ));
        }

        match liveness {
            AxRunLoopLiveness::Serviced => AxObserverContinuityDecision::Current(binding),
            AxRunLoopLiveness::Interrupted => AxObserverContinuityDecision::Broken(broken_from(
                binding,
                Some(current_run_loop_source_incarnation),
                AxObserverContinuityBreakReason::RunLoopInterrupted,
            )),
            AxRunLoopLiveness::Unverified => AxObserverContinuityDecision::Broken(broken_from(
                binding,
                Some(current_run_loop_source_incarnation),
                AxObserverContinuityBreakReason::RunLoopLivenessUnverified,
            )),
        }
    }

    /// Restores event continuity only with a genuinely newer observer lifecycle
    /// and a new run-loop source incarnation for the same application lifetime.
    pub fn rebind_after_break(
        &self,
        broken: AxBrokenObserverContinuity,
        fresh_observer_binding: AxObserverApplicationBinding,
        fresh_run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        liveness: AxRunLoopLiveness,
    ) -> Result<AxObserverContinuityBinding, AxObserverContinuityRebindError> {
        if fresh_observer_binding.application_incarnation() != &broken.application_incarnation {
            return Err(AxObserverContinuityRebindError::ApplicationIncarnationChanged);
        }

        if fresh_observer_binding.observer_creation_revision()
            <= broken.invalidated_observer_creation_revision
        {
            return Err(AxObserverContinuityRebindError::ObserverRevisionNotNewer);
        }

        if fresh_run_loop_source_incarnation == broken.previous_run_loop_source_incarnation {
            return Err(AxObserverContinuityRebindError::RunLoopSourceNotReplaced);
        }

        if liveness != AxRunLoopLiveness::Serviced {
            return Err(AxObserverContinuityRebindError::RunLoopNotServiced);
        }

        Ok(AxObserverContinuityBinding {
            observer_binding: fresh_observer_binding,
            run_loop_source_incarnation: fresh_run_loop_source_incarnation,
            continuity_revision: next_continuity_revision(),
        })
    }
}

fn broken_from(
    binding: AxObserverContinuityBinding,
    current_run_loop_source_incarnation: Option<AxRunLoopSourceIncarnation>,
    reason: AxObserverContinuityBreakReason,
) -> AxBrokenObserverContinuity {
    AxBrokenObserverContinuity {
        application_incarnation: binding.observer_binding.application_incarnation().clone(),
        previous_run_loop_source_incarnation: binding.run_loop_source_incarnation,
        current_run_loop_source_incarnation,
        invalidated_observer_creation_revision: binding.observer_binding.observer_creation_revision(),
        invalidated_continuity_revision: binding.continuity_revision,
        reason,
    }
}

fn next_continuity_revision() -> u64 {
    AX_OBSERVER_CONTINUITY_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}
