use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

use thiserror::Error;

use crate::{AxApplicationIncarnation, AxObserverApplicationBinding};

static AX_RUN_LOOP_SOURCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AX_OBSERVER_CONTINUITY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AX_CALLBACK_TRACKER_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static AX_CALLBACK_TRACKERS: OnceLock<Mutex<HashMap<usize, Weak<AxRunLoopCallbackState>>>> =
    OnceLock::new();

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

/// ABI-compatible callback type used by `AXObserverCreate`.
///
/// The provider callback intentionally ignores the raw AX element/notification
/// values here. M07 only needs proof that the exact observer/source lineage was
/// serviced by a real callback; semantic event decoding remains a separate
/// provider concern.
pub type AxObserverCallbackFn = unsafe extern "C" fn(
    *const c_void,
    *const c_void,
    *const c_void,
    *mut c_void,
);

#[derive(Debug)]
struct AxRunLoopCallbackState {
    application_incarnation: AxApplicationIncarnation,
    observer_creation_revision: u64,
    run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    delivered_callback_count: AtomicU64,
}

/// Provider-owned tracker for callbacks delivered by one exact AX observer and
/// one exact run-loop source incarnation.
///
/// The tracker owns no raw pointer into itself. `callback_refcon()` is an opaque
/// registry id; a late callback after tracker drop therefore becomes a no-op
/// instead of dereferencing freed memory.
#[derive(Debug)]
pub struct AxRunLoopCallbackTracker {
    tracker_id: usize,
    state: Arc<AxRunLoopCallbackState>,
}

impl AxRunLoopCallbackTracker {
    /// Callback function to pass to `AXObserverCreate`.
    pub const fn callback_function(&self) -> AxObserverCallbackFn {
        tracked_ax_observer_callback
    }

    /// Opaque refcon to pass to `AXObserverAddNotification` for notifications
    /// whose delivery should count toward this exact tracker.
    pub fn callback_refcon(&self) -> *mut c_void {
        self.tracker_id as *mut c_void
    }

    pub fn delivered_callback_count(&self) -> u64 {
        self.state.delivered_callback_count.load(Ordering::Acquire)
    }
}

impl Drop for AxRunLoopCallbackTracker {
    fn drop(&mut self) {
        let mut registry = callback_registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.remove(&self.tracker_id);
    }
}

/// Opaque provider-owned proof that a callback was actually delivered for one
/// exact observer/application/source lineage.
///
/// External callers cannot construct this evidence directly. They must obtain
/// it from [`AxObserverContinuityProvider::serviced_callback_evidence`] after
/// the provider callback tracker has observed at least one delivery.
///
/// ```compile_fail
/// use localview_macos_ax_provider::{AxApplicationIncarnation, AxRunLoopServiceEvidence};
///
/// let _forged = AxRunLoopServiceEvidence {
///     application_incarnation: AxApplicationIncarnation::new("forged", 1, 1),
///     observer_creation_revision: 1,
///     run_loop_source_incarnation: unsafe { core::mem::zeroed() },
///     delivered_callback_count: 1,
/// };
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct AxRunLoopServiceEvidence {
    application_incarnation: AxApplicationIncarnation,
    observer_creation_revision: u64,
    run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    delivered_callback_count: u64,
}

impl AxRunLoopServiceEvidence {
    pub fn application_incarnation(&self) -> &AxApplicationIncarnation {
        &self.application_incarnation
    }

    pub const fn observer_creation_revision(&self) -> u64 {
        self.observer_creation_revision
    }

    pub const fn run_loop_source_incarnation(&self) -> AxRunLoopSourceIncarnation {
        self.run_loop_source_incarnation
    }

    pub const fn delivered_callback_count(&self) -> u64 {
        self.delivered_callback_count
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxObserverContinuityStartError {
    #[error("serviced callback evidence does not match the observer/application/source lineage")]
    ServiceEvidenceMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxObserverContinuityBreakReason {
    RunLoopInterrupted,
    RunLoopLivenessUnverified,
    RunLoopSourceReplaced,
}

/// Live event-continuity authority. This token is intentionally move-only. It
/// can be created only from callback-backed service evidence and is consumed
/// when continuity is invalidated.
#[derive(Debug, PartialEq, Eq)]
pub struct AxObserverContinuityBinding {
    observer_binding: AxObserverApplicationBinding,
    run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    continuity_revision: u64,
    last_delivered_callback_count: u64,
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

    pub const fn last_delivered_callback_count(&self) -> u64 {
        self.last_delivered_callback_count
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
    invalidated_last_delivered_callback_count: u64,
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

    pub const fn invalidated_last_delivered_callback_count(&self) -> u64 {
        self.invalidated_last_delivered_callback_count
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxObserverContinuityRebindError {
    #[error("fresh observer belongs to a different application incarnation")]
    ApplicationIncarnationChanged,
    #[error("fresh observer creation revision is not newer than the broken lineage")]
    ObserverRevisionNotNewer,
    #[error("recovery attempted to reuse the invalidated run-loop source incarnation")]
    RunLoopSourceNotReplaced,
    #[error("serviced callback evidence does not match the fresh observer/application/source lineage")]
    ServiceEvidenceMismatch,
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

    /// Creates a provider-owned callback tracker bound to the exact observer
    /// authority and run-loop source incarnation that will receive callbacks.
    pub fn new_callback_tracker(
        &self,
        observer_binding: &AxObserverApplicationBinding,
        run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    ) -> AxRunLoopCallbackTracker {
        let tracker_id = AX_CALLBACK_TRACKER_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
        let state = Arc::new(AxRunLoopCallbackState {
            application_incarnation: observer_binding.application_incarnation().clone(),
            observer_creation_revision: observer_binding.observer_creation_revision(),
            run_loop_source_incarnation,
            delivered_callback_count: AtomicU64::new(0),
        });
        let mut registry = callback_registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.insert(tracker_id, Arc::downgrade(&state));
        AxRunLoopCallbackTracker { tracker_id, state }
    }

    /// Returns opaque serviced evidence only after the provider callback path
    /// has observed at least one real delivery for this tracker.
    pub fn serviced_callback_evidence(
        &self,
        tracker: &AxRunLoopCallbackTracker,
    ) -> Option<AxRunLoopServiceEvidence> {
        let delivered_callback_count = tracker.delivered_callback_count();
        if delivered_callback_count == 0 {
            return None;
        }
        Some(AxRunLoopServiceEvidence {
            application_incarnation: tracker.state.application_incarnation.clone(),
            observer_creation_revision: tracker.state.observer_creation_revision,
            run_loop_source_incarnation: tracker.state.run_loop_source_incarnation,
            delivered_callback_count,
        })
    }

    /// Establishes continuity only from provider-owned evidence produced by an
    /// actually delivered callback. Registration/source attachment by itself
    /// cannot mint continuity authority.
    pub fn activate(
        &self,
        observer_binding: AxObserverApplicationBinding,
        run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        service_evidence: AxRunLoopServiceEvidence,
    ) -> Result<AxObserverContinuityBinding, AxObserverContinuityStartError> {
        if !service_evidence_matches(
            &service_evidence,
            &observer_binding,
            run_loop_source_incarnation,
        ) {
            return Err(AxObserverContinuityStartError::ServiceEvidenceMismatch);
        }

        Ok(AxObserverContinuityBinding {
            observer_binding,
            run_loop_source_incarnation,
            continuity_revision: next_continuity_revision(),
            last_delivered_callback_count: service_evidence.delivered_callback_count,
        })
    }

    /// Consumes live continuity and creates a fail-closed tombstone. A changed
    /// source incarnation always wins as the factual break reason; otherwise
    /// the caller may report interruption or inability to verify servicing.
    /// There is intentionally no positive `Serviced` input on this path.
    pub fn break_continuity(
        &self,
        binding: AxObserverContinuityBinding,
        current_run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        observed_reason: AxObserverContinuityBreakReason,
    ) -> AxBrokenObserverContinuity {
        let reason = if current_run_loop_source_incarnation != binding.run_loop_source_incarnation {
            AxObserverContinuityBreakReason::RunLoopSourceReplaced
        } else if observed_reason == AxObserverContinuityBreakReason::RunLoopSourceReplaced {
            AxObserverContinuityBreakReason::RunLoopLivenessUnverified
        } else {
            observed_reason
        };

        broken_from(
            binding,
            Some(current_run_loop_source_incarnation),
            reason,
        )
    }

    /// Restores event continuity only with a genuinely newer observer lifecycle,
    /// a new source incarnation, and callback-backed service evidence bound to
    /// that exact fresh lineage.
    pub fn rebind_after_break(
        &self,
        broken: AxBrokenObserverContinuity,
        fresh_observer_binding: AxObserverApplicationBinding,
        fresh_run_loop_source_incarnation: AxRunLoopSourceIncarnation,
        service_evidence: AxRunLoopServiceEvidence,
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

        if !service_evidence_matches(
            &service_evidence,
            &fresh_observer_binding,
            fresh_run_loop_source_incarnation,
        ) {
            return Err(AxObserverContinuityRebindError::ServiceEvidenceMismatch);
        }

        Ok(AxObserverContinuityBinding {
            observer_binding: fresh_observer_binding,
            run_loop_source_incarnation: fresh_run_loop_source_incarnation,
            continuity_revision: next_continuity_revision(),
            last_delivered_callback_count: service_evidence.delivered_callback_count,
        })
    }
}

fn callback_registry() -> &'static Mutex<HashMap<usize, Weak<AxRunLoopCallbackState>>> {
    AX_CALLBACK_TRACKERS.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe extern "C" fn tracked_ax_observer_callback(
    _observer: *const c_void,
    _element: *const c_void,
    _notification: *const c_void,
    refcon: *mut c_void,
) {
    let tracker_id = refcon as usize;
    if tracker_id == 0 {
        return;
    }

    let state = {
        let registry = callback_registry()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.get(&tracker_id).and_then(Weak::upgrade)
    };

    if let Some(state) = state {
        state.delivered_callback_count.fetch_add(1, Ordering::AcqRel);
    }
}

fn service_evidence_matches(
    service_evidence: &AxRunLoopServiceEvidence,
    observer_binding: &AxObserverApplicationBinding,
    run_loop_source_incarnation: AxRunLoopSourceIncarnation,
) -> bool {
    service_evidence.delivered_callback_count > 0
        && service_evidence.application_incarnation == *observer_binding.application_incarnation()
        && service_evidence.observer_creation_revision
            == observer_binding.observer_creation_revision()
        && service_evidence.run_loop_source_incarnation == run_loop_source_incarnation
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
        invalidated_last_delivered_callback_count: binding.last_delivered_callback_count,
        reason,
    }
}

fn next_continuity_revision() -> u64 {
    AX_OBSERVER_CONTINUITY_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1
}
