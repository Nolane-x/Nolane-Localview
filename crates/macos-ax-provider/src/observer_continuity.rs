use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
};

#[cfg(any(target_os = "macos", test))]
use std::ffi::c_void;

use thiserror::Error;

use crate::{AxApplicationIncarnation, AxObserverApplicationBinding};

static AX_RUN_LOOP_SOURCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AX_OBSERVER_CONTINUITY_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AX_CALLBACK_TRACKER_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static AX_CALLBACK_TRACKERS: OnceLock<Mutex<HashMap<usize, Weak<AxRunLoopCallbackState>>>> =
    OnceLock::new();

#[cfg(target_os = "macos")]
const AX_ERROR_SUCCESS: i32 = 0;

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

#[cfg(target_os = "macos")]
type AxObserverCallbackFn = unsafe extern "C" fn(
    *const c_void,
    *const c_void,
    *const c_void,
    *mut c_void,
);

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXObserverCreate(
        application: i32,
        callback: AxObserverCallbackFn,
        out_observer: *mut *const c_void,
    ) -> i32;
    fn AXObserverAddNotification(
        observer: *const c_void,
        element: *const c_void,
        notification: *const c_void,
        refcon: *mut c_void,
    ) -> i32;
    fn AXObserverGetRunLoopSource(observer: *const c_void) -> *const c_void;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}

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
/// The tracker deliberately exposes only read-only delivery state. Shipping
/// callers cannot obtain either the native callback function or the refcon used
/// by Accessibility.framework, so they cannot synthesize callback delivery.
#[derive(Debug)]
pub struct AxRunLoopCallbackTracker {
    tracker_id: usize,
    state: Arc<AxRunLoopCallbackState>,
}

impl AxRunLoopCallbackTracker {
    pub fn delivered_callback_count(&self) -> u64 {
        self.state.delivered_callback_count.load(Ordering::Acquire)
    }

    #[cfg(any(target_os = "macos", test))]
    fn callback_refcon(&self) -> *mut c_void {
        self.tracker_id as *mut c_void
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

/// Native AX observer whose callback and registration refcon remain owned by
/// this provider crate.
///
/// The wrapper intentionally does not expose the raw AXObserverRef or callback
/// function. This keeps positive run-loop service authority on the provider
/// side while still allowing the runtime to attach the returned CFRunLoopSource
/// to its chosen run loop.
#[derive(Debug)]
pub struct AxTrackedObserver {
    tracker: AxRunLoopCallbackTracker,
    #[cfg(target_os = "macos")]
    observer: *const c_void,
}

impl AxTrackedObserver {
    pub fn delivered_callback_count(&self) -> u64 {
        self.tracker.delivered_callback_count()
    }

    pub fn callback_tracker(&self) -> &AxRunLoopCallbackTracker {
        &self.tracker
    }

    /// Register one AX notification while keeping the callback refcon private.
    ///
    /// # Safety
    /// `element` and `notification` must be valid macOS Accessibility/CoreFoundation
    /// objects for the duration of the call.
    #[cfg(target_os = "macos")]
    pub unsafe fn add_notification(
        &self,
        element: *const c_void,
        notification: *const c_void,
    ) -> i32 {
        unsafe {
            AXObserverAddNotification(
                self.observer,
                element,
                notification,
                self.tracker.callback_refcon(),
            )
        }
    }

    /// Borrow the native CFRunLoopSource associated with this observer.
    /// The pointer remains owned by Accessibility.framework and must not outlive
    /// this `AxTrackedObserver`.
    #[cfg(target_os = "macos")]
    pub fn run_loop_source(&self) -> *const c_void {
        unsafe { AXObserverGetRunLoopSource(self.observer) }
    }
}

#[cfg(target_os = "macos")]
impl Drop for AxTrackedObserver {
    fn drop(&mut self) {
        if !self.observer.is_null() {
            unsafe { CFRelease(self.observer) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxTrackedObserverCreateError {
    #[error("AXObserverCreate failed with AXError {0}")]
    CreateFailed(i32),
    #[error("AXObserverCreate reported success but returned a null observer")]
    NullObserver,
    #[error("native AX observer creation is available only on macOS")]
    PlatformUnsupported,
}

/// Opaque provider-owned proof that a callback was actually delivered for one
/// exact observer/application/source lineage.
///
/// External callers cannot construct this evidence directly. They must obtain
/// it from [`AxObserverContinuityProvider::serviced_callback_evidence`] after
/// the provider-owned native callback path has observed at least one delivery.
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

    /// Creates a native observer whose callback and registration refcon are
    /// owned end-to-end by this provider. The PID comes from the exact observer
    /// application binding rather than from a caller-supplied side channel.
    pub fn create_tracked_observer(
        &self,
        observer_binding: &AxObserverApplicationBinding,
        run_loop_source_incarnation: AxRunLoopSourceIncarnation,
    ) -> Result<AxTrackedObserver, AxTrackedObserverCreateError> {
        let tracker = self.new_callback_tracker(observer_binding, run_loop_source_incarnation);

        #[cfg(target_os = "macos")]
        {
            let mut observer: *const c_void = std::ptr::null();
            let error = unsafe {
                AXObserverCreate(
                    observer_binding.application_incarnation().pid(),
                    tracked_ax_observer_callback,
                    &mut observer,
                )
            };
            if error != AX_ERROR_SUCCESS {
                return Err(AxTrackedObserverCreateError::CreateFailed(error));
            }
            if observer.is_null() {
                return Err(AxTrackedObserverCreateError::NullObserver);
            }
            Ok(AxTrackedObserver { tracker, observer })
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = tracker;
            Err(AxTrackedObserverCreateError::PlatformUnsupported)
        }
    }

    /// Creates provider-owned callback state for one exact observer/source
    /// lineage. This does not itself mint positive continuity authority.
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

    /// Returns opaque serviced evidence only after the private provider callback
    /// path has observed at least one delivery for this tracker.
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

    pub fn serviced_observer_evidence(
        &self,
        observer: &AxTrackedObserver,
    ) -> Option<AxRunLoopServiceEvidence> {
        self.serviced_callback_evidence(observer.callback_tracker())
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

#[cfg(any(target_os = "macos", test))]
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

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, ptr};

    use super::*;
    use crate::AxObserverApplicationBindingProvider;

    fn app() -> AxApplicationIncarnation {
        AxApplicationIncarnation::new("com.nolane.localview.m07-seed", 7070, 7001)
    }

    fn simulate_provider_callback(tracker: &AxRunLoopCallbackTracker) {
        unsafe {
            tracked_ax_observer_callback(
                ptr::null::<c_void>(),
                ptr::null::<c_void>(),
                ptr::null::<c_void>(),
                tracker.callback_refcon(),
            );
        }
    }

    #[test]
    fn registration_state_without_callback_cannot_mint_service_evidence() {
        let observers = AxObserverApplicationBindingProvider::new();
        let continuity = AxObserverContinuityProvider::new();
        let binding = observers.bind_current(app());
        let source = continuity.new_run_loop_source_incarnation();
        let tracker = continuity.new_callback_tracker(&binding, source);

        assert_eq!(tracker.delivered_callback_count(), 0);
        assert!(continuity.serviced_callback_evidence(&tracker).is_none());
    }

    #[test]
    fn private_provider_callback_mints_exact_lineage_evidence() {
        let observers = AxObserverApplicationBindingProvider::new();
        let continuity = AxObserverContinuityProvider::new();
        let binding = observers.bind_current(app());
        let observer_revision = binding.observer_creation_revision();
        let source = continuity.new_run_loop_source_incarnation();
        let tracker = continuity.new_callback_tracker(&binding, source);

        simulate_provider_callback(&tracker);
        let evidence = continuity
            .serviced_callback_evidence(&tracker)
            .expect("private callback must mint service evidence");
        assert_eq!(evidence.observer_creation_revision(), observer_revision);
        assert_eq!(evidence.run_loop_source_incarnation(), source);
        assert_eq!(evidence.delivered_callback_count(), 1);

        let live = continuity
            .activate(binding, source, evidence)
            .expect("exact callback evidence must establish continuity");
        assert_eq!(live.last_delivered_callback_count(), 1);
    }

    #[test]
    fn interruption_tombstones_live_continuity() {
        let observers = AxObserverApplicationBindingProvider::new();
        let continuity = AxObserverContinuityProvider::new();
        let binding = observers.bind_current(app());
        let source = continuity.new_run_loop_source_incarnation();
        let tracker = continuity.new_callback_tracker(&binding, source);
        simulate_provider_callback(&tracker);
        let evidence = continuity.serviced_callback_evidence(&tracker).unwrap();
        let live = continuity.activate(binding, source, evidence).unwrap();
        let revision = live.continuity_revision();

        let broken = continuity.break_continuity(
            live,
            source,
            AxObserverContinuityBreakReason::RunLoopInterrupted,
        );
        assert_eq!(broken.invalidated_continuity_revision(), revision);
        assert_eq!(
            broken.reason(),
            AxObserverContinuityBreakReason::RunLoopInterrupted
        );
        assert!(broken.requires_snapshot_reconciliation());
        assert!(broken.requires_observer_recreation());
    }

    #[test]
    fn recovery_requires_fresh_observer_source_and_matching_callback_evidence() {
        let observers = AxObserverApplicationBindingProvider::new();
        let continuity = AxObserverContinuityProvider::new();
        let current_app = app();

        let old_binding = observers.bind_current(current_app.clone());
        let old_source = continuity.new_run_loop_source_incarnation();
        let old_tracker = continuity.new_callback_tracker(&old_binding, old_source);
        simulate_provider_callback(&old_tracker);
        let old_evidence = continuity.serviced_callback_evidence(&old_tracker).unwrap();
        let old_live = continuity
            .activate(old_binding, old_source, old_evidence)
            .unwrap();
        let old_observer_revision = old_live.observer_creation_revision();
        let broken = continuity.break_continuity(
            old_live,
            old_source,
            AxObserverContinuityBreakReason::RunLoopInterrupted,
        );

        let fresh_binding = observers.bind_current(current_app.clone());
        let fresh_source = continuity.new_run_loop_source_incarnation();
        let fresh_tracker = continuity.new_callback_tracker(&fresh_binding, fresh_source);
        assert!(continuity.serviced_callback_evidence(&fresh_tracker).is_none());
        simulate_provider_callback(&fresh_tracker);
        let fresh_evidence = continuity.serviced_callback_evidence(&fresh_tracker).unwrap();
        let restored = continuity
            .rebind_after_break(broken, fresh_binding, fresh_source, fresh_evidence)
            .expect("fresh exact lineage must restore continuity");
        assert!(restored.observer_creation_revision() > old_observer_revision);
        assert_eq!(restored.run_loop_source_incarnation(), fresh_source);

        let wrong_binding = observers.bind_current(current_app);
        let wrong_source = continuity.new_run_loop_source_incarnation();
        let wrong_tracker = continuity.new_callback_tracker(&wrong_binding, wrong_source);
        simulate_provider_callback(&wrong_tracker);
        assert!(continuity.serviced_callback_evidence(&wrong_tracker).is_some());
    }
}
