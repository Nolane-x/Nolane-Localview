//! macOS Accessibility permission authority boundary for LocalView V4.3.
//!
//! M01 establishes provider-owned live Accessibility trust observation before
//! semantic-control admission. M02 extends that authority boundary to dispatch:
//! a permit admitted under an earlier trusted revision never authorizes a
//! consequential AX dispatch by itself; dispatch rechecks effective OS trust
//! through a fresh process and either refreshes authority or fails closed if
//! permission was revoked or cannot be observed conclusively.
//! Application attachment, AX snapshots, observers, concrete AX action
//! dispatch, and capture/input fallback remain outside this slice.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use thiserror::Error;

static AX_PERMISSION_CHECK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_API_DISABLED: i32 = -25211;
const AX_PERMISSION_PROBE_ARG: &str = "--localview-internal-ax-permission-probe-v1";
const AX_PERMISSION_PROBE_ENV: &str = "LOCALVIEW_INTERNAL_AX_PERMISSION_PROBE_V1";
const AX_PERMISSION_PROBE_ENV_VALUE: &str = "1";
const AX_PERMISSION_PROBE_PREFIX: &str = "LOCALVIEW_AX_PERMISSION_STATE_V1=";
const AX_PERMISSION_PROBE_TIMEOUT: Duration = Duration::from_millis(2_000);
const AX_PERMISSION_PROBE_POLL: Duration = Duration::from_millis(10);

/// Current knowledge about macOS Accessibility trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxPermissionState {
    Trusted,
    Untrusted,
    Unknown,
}

/// Immutable OS-backed observation of Accessibility permission at one check
/// sequence.
///
/// The constructor is intentionally private. External callers can inspect a
/// revision returned by [`AxPermissionProvider`], but cannot manufacture a
/// `Trusted` revision and feed it back as semantic-control authority.
///
/// ```compile_fail
/// use localview_macos_ax_provider::{AxPermissionRevision, AxPermissionState};
///
/// let _forged = AxPermissionRevision::observed(
///     AxPermissionState::Trusted,
///     u64::MAX,
///     false,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxPermissionRevision {
    state: AxPermissionState,
    check_sequence: u64,
    prompt_requested: bool,
}

impl AxPermissionRevision {
    const fn observed(
        state: AxPermissionState,
        check_sequence: u64,
        prompt_requested: bool,
    ) -> Self {
        Self {
            state,
            check_sequence,
            prompt_requested,
        }
    }

    pub const fn state(&self) -> AxPermissionState {
        self.state
    }

    pub const fn check_sequence(&self) -> u64 {
        self.check_sequence
    }

    pub const fn prompt_requested(&self) -> bool {
        self.prompt_requested
    }
}

/// Capability token proving the permission revision that admitted AX semantic
/// control. The token is intentionally opaque and cannot directly authorize a
/// dispatch; callers must submit it to [`AxPermissionProvider::semantic_dispatch_decision`]
/// for a fresh OS-backed permission fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticControlPermit {
    permission_check_sequence: u64,
}

impl AxSemanticControlPermit {
    pub const fn permission_check_sequence(&self) -> u64 {
        self.permission_check_sequence
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AxPermissionError {
    #[error("macOS Accessibility permission is required")]
    PermissionRequired,
    #[error("macOS Accessibility permission was revoked after semantic-control admission")]
    PermissionRevoked,
    #[error("macOS Accessibility permission state is unknown")]
    PermissionUnknown,
}

/// Result of one provider-owned semantic-control authority check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSemanticControlOutcome {
    Authorized(AxSemanticControlPermit),
    Denied(AxPermissionError),
}

/// One coherent permission observation plus the semantic-control admission
/// decision derived from that same observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticControlDecision {
    revision: AxPermissionRevision,
    outcome: AxSemanticControlOutcome,
}

impl AxSemanticControlDecision {
    pub const fn revision(&self) -> AxPermissionRevision {
        self.revision
    }

    pub const fn outcome(&self) -> AxSemanticControlOutcome {
        self.outcome
    }

    pub const fn permit(&self) -> Option<AxSemanticControlPermit> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(permit) => Some(permit),
            AxSemanticControlOutcome::Denied(_) => None,
        }
    }

    pub const fn denial(&self) -> Option<AxPermissionError> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(_) => None,
            AxSemanticControlOutcome::Denied(error) => Some(error),
        }
    }
}

/// Dispatch-time permission fence. It records which previously admitted
/// permission revision was presented, the newer live revision observed before
/// dispatch, and the refreshed or denied authority derived from that new
/// observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSemanticDispatchDecision {
    admitted_permission_check_sequence: u64,
    revision: AxPermissionRevision,
    outcome: AxSemanticControlOutcome,
}

impl AxSemanticDispatchDecision {
    pub const fn admitted_permission_check_sequence(&self) -> u64 {
        self.admitted_permission_check_sequence
    }

    pub const fn revision(&self) -> AxPermissionRevision {
        self.revision
    }

    pub const fn outcome(&self) -> AxSemanticControlOutcome {
        self.outcome
    }

    pub const fn permit(&self) -> Option<AxSemanticControlPermit> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(permit) => Some(permit),
            AxSemanticControlOutcome::Denied(_) => None,
        }
    }

    pub const fn denial(&self) -> Option<AxPermissionError> {
        match self.outcome {
            AxSemanticControlOutcome::Authorized(_) => None,
            AxSemanticControlOutcome::Denied(error) => Some(error),
        }
    }
}

/// Production boundary for observing and authorizing macOS Accessibility use.
///
/// Admission reads the current process directly. Consequential dispatch uses a
/// fresh child process because macOS can retain a stale Accessibility trust
/// result in a process that was already running when TCC authorization changed.
/// Production hosts must call [`run_permission_probe_if_requested`] before
/// normal application startup so the default self-reexec probe can terminate
/// without constructing the UI/runtime.
#[derive(Debug, Clone)]
pub struct AxPermissionProvider {
    dispatch_probe_executable: Option<PathBuf>,
}

impl Default for AxPermissionProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AxPermissionProvider {
    pub const fn new() -> Self {
        Self {
            dispatch_probe_executable: None,
        }
    }

    /// Overrides the executable used for the fresh-process dispatch probe.
    ///
    /// This is primarily for provider integration tests and narrowly scoped
    /// hosts. Production desktop use normally leaves this unset so LocalView
    /// re-executes its own current executable and therefore preserves the host
    /// identity whose TCC capability is being fenced.
    pub fn with_dispatch_probe_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            dispatch_probe_executable: Some(path.into()),
        }
    }

    /// Reads current-process trust from the operating system.
    ///
    /// This is suitable for initial admission and diagnostics. Dispatch does
    /// not use this same-process observation because macOS may keep it stale
    /// after a mid-session TCC change.
    pub fn current_permission_revision(
        &self,
        prompt_requested: bool,
    ) -> AxPermissionRevision {
        permission_revision(platform_permission_state(), prompt_requested)
    }

    /// Takes an OS-backed permission observation and derives semantic-control
    /// admission authority from that exact revision. Callers never submit a
    /// revision to this method, so a fabricated `Trusted` state cannot cross
    /// the authority boundary.
    pub fn semantic_control_decision(
        &self,
        prompt_requested: bool,
    ) -> AxSemanticControlDecision {
        decision_from_revision(self.current_permission_revision(prompt_requested))
    }

    /// Revalidates Accessibility trust immediately before semantic dispatch.
    ///
    /// The previously admitted permit is evidence that admission once
    /// succeeded; it is not dispatch authority. A bounded fresh process probes
    /// effective TCC state. Spawn failure, timeout, malformed output, or any
    /// other inconclusive probe becomes `Unknown`, which fails closed rather
    /// than reusing the old permit.
    pub fn semantic_dispatch_decision(
        &self,
        admitted_permit: AxSemanticControlPermit,
    ) -> AxSemanticDispatchDecision {
        let state = self.fresh_dispatch_permission_state();
        dispatch_decision_from_revision(
            admitted_permit,
            permission_revision(state, false),
        )
    }

    fn fresh_dispatch_permission_state(&self) -> AxPermissionState {
        let executable = match self.dispatch_probe_executable.as_deref() {
            Some(path) => path.to_path_buf(),
            None => match std::env::current_exe() {
                Ok(path) => path,
                Err(_) => return AxPermissionState::Unknown,
            },
        };

        run_fresh_permission_probe(&executable)
    }
}

/// Handles the internal self-reexec permission-probe mode.
///
/// A macOS host using [`AxPermissionProvider::new`] must call this before
/// constructing its normal runtime. The two-part marker (reserved argument +
/// reserved environment variable) avoids accidentally entering probe mode from
/// an ordinary launch. Probe mode prints one versioned machine-readable line
/// and returns `true`; the host should then exit immediately.
pub fn run_permission_probe_if_requested() -> bool {
    let env_enabled = std::env::var(AX_PERMISSION_PROBE_ENV)
        .ok()
        .as_deref()
        == Some(AX_PERMISSION_PROBE_ENV_VALUE);
    let arg_enabled = std::env::args_os().any(|arg| arg == AX_PERMISSION_PROBE_ARG);

    if !(env_enabled && arg_enabled) {
        return false;
    }

    println!(
        "{AX_PERMISSION_PROBE_PREFIX}{}",
        encode_permission_state(platform_permission_state())
    );
    true
}

fn permission_revision(
    state: AxPermissionState,
    prompt_requested: bool,
) -> AxPermissionRevision {
    let check_sequence = AX_PERMISSION_CHECK_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    AxPermissionRevision::observed(state, check_sequence, prompt_requested)
}

fn run_fresh_permission_probe(executable: &Path) -> AxPermissionState {
    let mut child = match Command::new(executable)
        .arg(AX_PERMISSION_PROBE_ARG)
        .env(AX_PERMISSION_PROBE_ENV, AX_PERMISSION_PROBE_ENV_VALUE)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return AxPermissionState::Unknown,
    };

    let deadline = Instant::now() + AX_PERMISSION_PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return AxPermissionState::Unknown;
                }

                let mut stdout = String::new();
                let Some(mut pipe) = child.stdout.take() else {
                    return AxPermissionState::Unknown;
                };
                if pipe.read_to_string(&mut stdout).is_err() {
                    return AxPermissionState::Unknown;
                }
                return parse_permission_probe_output(&stdout)
                    .unwrap_or(AxPermissionState::Unknown);
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(AX_PERMISSION_PROBE_POLL);
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return AxPermissionState::Unknown;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return AxPermissionState::Unknown;
            }
        }
    }
}

fn encode_permission_state(state: AxPermissionState) -> &'static str {
    match state {
        AxPermissionState::Trusted => "trusted",
        AxPermissionState::Untrusted => "untrusted",
        AxPermissionState::Unknown => "unknown",
    }
}

fn parse_permission_probe_output(output: &str) -> Option<AxPermissionState> {
    let mut states = output.lines().filter_map(|line| {
        let value = line.strip_prefix(AX_PERMISSION_PROBE_PREFIX)?;
        match value {
            "trusted" => Some(AxPermissionState::Trusted),
            "untrusted" => Some(AxPermissionState::Untrusted),
            "unknown" => Some(AxPermissionState::Unknown),
            _ => None,
        }
    });

    let state = states.next()?;
    if states.next().is_some() {
        return None;
    }
    Some(state)
}

fn decision_from_revision(revision: AxPermissionRevision) -> AxSemanticControlDecision {
    let outcome = match revision.state {
        AxPermissionState::Trusted => {
            AxSemanticControlOutcome::Authorized(AxSemanticControlPermit {
                permission_check_sequence: revision.check_sequence,
            })
        }
        AxPermissionState::Untrusted => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRequired)
        }
        AxPermissionState::Unknown => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionUnknown)
        }
    };

    AxSemanticControlDecision { revision, outcome }
}

fn dispatch_decision_from_revision(
    admitted_permit: AxSemanticControlPermit,
    revision: AxPermissionRevision,
) -> AxSemanticDispatchDecision {
    let outcome = match revision.state {
        AxPermissionState::Trusted => {
            AxSemanticControlOutcome::Authorized(AxSemanticControlPermit {
                permission_check_sequence: revision.check_sequence,
            })
        }
        AxPermissionState::Untrusted => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRevoked)
        }
        AxPermissionState::Unknown => {
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionUnknown)
        }
    };

    AxSemanticDispatchDecision {
        admitted_permission_check_sequence: admitted_permit.permission_check_sequence,
        revision,
        outcome,
    }
}

fn classify_permission_observation(
    process_trusted: bool,
    messaging_error: i32,
) -> AxPermissionState {
    if !process_trusted || messaging_error == AX_ERROR_API_DISABLED {
        AxPermissionState::Untrusted
    } else if messaging_error == AX_ERROR_SUCCESS {
        AxPermissionState::Trusted
    } else {
        // A trust boolean without a usable AX messaging path is not strong
        // enough to mint consequential semantic-control authority.
        AxPermissionState::Unknown
    }
}

#[cfg(target_os = "macos")]
fn platform_permission_state() -> AxPermissionState {
    use std::{ffi::c_void, ptr};

    type AxUiElementRef = *const c_void;
    type CfArrayRef = *const c_void;
    type CfTypeRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        // AXUIElement.h declares `Boolean AXIsProcessTrusted(void)`. Core
        // Foundation's Boolean is an unsigned byte, so bind it as `u8` rather
        // than relying on Rust `bool` FFI layout.
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
        fn AXUIElementCopyAttributeNames(
            element: AxUiElementRef,
            names: *mut CfArrayRef,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: CfTypeRef);
    }

    let process_trusted = unsafe { AXIsProcessTrusted() } != 0;
    if !process_trusted {
        return AxPermissionState::Untrusted;
    }

    // AXIsProcessTrusted is necessary but not sufficient for a dispatch fence.
    // The fresh-process boundary above fixes process-lifetime TCC staleness;
    // this non-mutating AX messaging probe additionally refuses authority if
    // the current process cannot use the API despite a favorable trust bit.
    let system_wide = unsafe { AXUIElementCreateSystemWide() };
    if system_wide.is_null() {
        return AxPermissionState::Unknown;
    }

    let mut names: CfArrayRef = ptr::null();
    let messaging_error = unsafe { AXUIElementCopyAttributeNames(system_wide, &mut names) };
    if !names.is_null() {
        unsafe { CFRelease(names.cast()) };
    }
    unsafe { CFRelease(system_wide.cast()) };

    classify_permission_observation(process_trusted, messaging_error)
}

#[cfg(not(target_os = "macos"))]
fn platform_permission_state() -> AxPermissionState {
    // A non-macOS build cannot truthfully answer an AX trust question.
    AxPermissionState::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_permission_is_typed_and_never_mints_a_permit() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Untrusted,
            1,
            false,
        ));

        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionRequired));
    }

    #[test]
    fn unknown_permission_is_distinct_from_explicit_denial() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Unknown,
            2,
            false,
        ));

        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionUnknown));
    }

    #[test]
    fn prompt_requested_does_not_override_untrusted_state() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Untrusted,
            3,
            true,
        ));

        assert!(decision.revision().prompt_requested());
        assert_eq!(decision.permit(), None);
        assert_eq!(decision.denial(), Some(AxPermissionError::PermissionRequired));
    }

    #[test]
    fn trusted_permission_mints_a_permit_bound_to_the_observed_revision() {
        let decision = decision_from_revision(AxPermissionRevision::observed(
            AxPermissionState::Trusted,
            41,
            false,
        ));

        let permit = decision.permit().expect("trusted observation should authorize");
        assert_eq!(permit.permission_check_sequence(), 41);
        assert_eq!(decision.denial(), None);
    }

    #[test]
    fn revoked_permission_invalidates_prior_permit_at_dispatch() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 50,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Untrusted, 51, false),
        );

        assert_eq!(dispatch.admitted_permission_check_sequence(), 50);
        assert_eq!(dispatch.revision().check_sequence(), 51);
        assert_eq!(dispatch.permit(), None);
        assert_eq!(dispatch.denial(), Some(AxPermissionError::PermissionRevoked));
    }

    #[test]
    fn still_trusted_dispatch_refreshes_authority_to_newer_revision() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 60,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Trusted, 61, false),
        );

        let refreshed = dispatch
            .permit()
            .expect("still-trusted dispatch should refresh authority");
        assert_eq!(dispatch.admitted_permission_check_sequence(), 60);
        assert_eq!(refreshed.permission_check_sequence(), 61);
        assert_eq!(dispatch.denial(), None);
    }

    #[test]
    fn unknown_dispatch_state_never_reuses_prior_permit() {
        let admitted = AxSemanticControlPermit {
            permission_check_sequence: 70,
        };
        let dispatch = dispatch_decision_from_revision(
            admitted,
            AxPermissionRevision::observed(AxPermissionState::Unknown, 71, false),
        );

        assert_eq!(dispatch.permit(), None);
        assert_eq!(dispatch.denial(), Some(AxPermissionError::PermissionUnknown));
    }

    #[test]
    fn permission_probe_protocol_accepts_exact_single_state() {
        assert_eq!(
            parse_permission_probe_output("LOCALVIEW_AX_PERMISSION_STATE_V1=trusted\n"),
            Some(AxPermissionState::Trusted)
        );
        assert_eq!(
            parse_permission_probe_output("LOCALVIEW_AX_PERMISSION_STATE_V1=untrusted\n"),
            Some(AxPermissionState::Untrusted)
        );
        assert_eq!(
            parse_permission_probe_output("LOCALVIEW_AX_PERMISSION_STATE_V1=unknown\n"),
            Some(AxPermissionState::Unknown)
        );
    }

    #[test]
    fn permission_probe_protocol_rejects_missing_duplicate_or_invalid_state() {
        assert_eq!(parse_permission_probe_output("trusted\n"), None);
        assert_eq!(
            parse_permission_probe_output(
                "LOCALVIEW_AX_PERMISSION_STATE_V1=trusted\nLOCALVIEW_AX_PERMISSION_STATE_V1=trusted\n"
            ),
            None
        );
        assert_eq!(
            parse_permission_probe_output("LOCALVIEW_AX_PERMISSION_STATE_V1=maybe\n"),
            None
        );
    }

    #[test]
    fn api_disabled_overrides_a_stale_trusted_boolean() {
        assert_eq!(
            classify_permission_observation(true, AX_ERROR_API_DISABLED),
            AxPermissionState::Untrusted
        );
    }

    #[test]
    fn usable_ax_messaging_confirms_trusted_permission() {
        assert_eq!(
            classify_permission_observation(true, AX_ERROR_SUCCESS),
            AxPermissionState::Trusted
        );
    }

    #[test]
    fn untrusted_boolean_never_becomes_trusted_from_messaging_success() {
        assert_eq!(
            classify_permission_observation(false, AX_ERROR_SUCCESS),
            AxPermissionState::Untrusted
        );
    }

    #[test]
    fn unexpected_messaging_failure_is_inconclusive_not_authorized() {
        assert_eq!(
            classify_permission_observation(true, -25204),
            AxPermissionState::Unknown
        );
    }
}
