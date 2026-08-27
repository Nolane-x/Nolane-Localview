#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::Duration,
};

use chrono::Utc;
use localview_chromium::{
    execute_ephemeral, validate_loopback_url, ChromiumExecutionPolicy, ChromiumExecutorError,
};
use localview_evidence::{EvidenceDraft, EvidenceKind, Provenance, UncertaintyClass};
use localview_protocol::SessionId;
use localview_resource_governor::{ResourceAdmissionDenial, ResourceWorkKind};
use localview_sessions::SessionManager;
use url::Url;
use uuid::Uuid;

use crate::{
    resource_runtime::{governor as resource_governor, runtime_resource_governor_for_sessions},
    ControlState,
};

const DEFAULT_CHROMIUM_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_CHROMIUM_STDOUT_BYTES: usize = 64 * 1024;
const DEFAULT_CHROMIUM_STDERR_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct ChromiumExecutorConfig {
    executable: PathBuf,
    policy: ChromiumExecutionPolicy,
}

#[derive(Debug, Clone)]
pub(crate) struct ChromiumCompatibilityReceipt {
    pub(crate) target: String,
    pub(crate) exit_code: i32,
    pub(crate) stdout_total_bytes: usize,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_total_bytes: usize,
    pub(crate) stderr_truncated: bool,
    pub(crate) evidence_id: String,
}

#[derive(Debug)]
pub(crate) enum ChromiumRuntimeError {
    ExecutorUnavailable,
    SessionNotFound,
    InvalidTarget,
    ResourceGovernor(ResourceAdmissionDenial),
    Executor(ChromiumExecutorError),
    NonZeroExit(Option<i32>),
}

#[derive(Debug)]
struct ChromiumEntry {
    owner: Weak<SessionManager>,
    config: ChromiumExecutorConfig,
}

type ChromiumRegistry = HashMap<usize, ChromiumEntry>;

static CHROMIUM_EXECUTORS: OnceLock<Mutex<ChromiumRegistry>> = OnceLock::new();

#[doc(hidden)]
pub fn configure_chromium_executor_for_sessions(
    sessions: &Arc<SessionManager>,
    executable: PathBuf,
    temp_root: PathBuf,
) {
    let key = Arc::as_ptr(sessions) as usize;
    let registry = CHROMIUM_EXECUTORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.insert(
        key,
        ChromiumEntry {
            owner: Arc::downgrade(sessions),
            config: ChromiumExecutorConfig {
                executable,
                policy: ChromiumExecutionPolicy {
                    timeout: DEFAULT_CHROMIUM_TIMEOUT,
                    max_stdout_bytes: DEFAULT_CHROMIUM_STDOUT_BYTES,
                    max_stderr_bytes: DEFAULT_CHROMIUM_STDERR_BYTES,
                    temp_root,
                },
            },
        },
    );
    let _ = runtime_resource_governor_for_sessions(sessions);
}

pub(crate) async fn execute_compatibility_probe(
    state: &ControlState,
    id: SessionId,
    revision: Option<&str>,
    region: Option<String>,
    timeout_cap: Option<Duration>,
) -> Result<ChromiumCompatibilityReceipt, ChromiumRuntimeError> {
    let config = config(state).ok_or(ChromiumRuntimeError::ExecutorUnavailable)?;
    let target = resolve_target(state, id).await?;
    let public_target = private_safe_route_identity(&target);
    let reservation_id = Uuid::new_v4();
    let _reservation = resource_governor(state)
        .reserve(
            id.to_string(),
            reservation_id.to_string(),
            ResourceWorkKind::Chromium,
        )
        .map_err(ChromiumRuntimeError::ResourceGovernor)?;

    let mut policy = config.policy;
    if let Some(timeout_cap) = timeout_cap {
        policy.timeout = policy.timeout.min(timeout_cap.max(Duration::from_millis(1)));
    }
    let execution = execute_ephemeral(&config.executable, &target, &policy)
        .await
        .map_err(ChromiumRuntimeError::Executor)?;
    let Some(exit_code) = execution.exit_code.filter(|code| *code == 0) else {
        return Err(ChromiumRuntimeError::NonZeroExit(execution.exit_code));
    };

    let captured_at = Utc::now();
    let evidence = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Contract,
            session_id: id,
            region,
            payload: serde_json::json!({
                "probe": "page_load_dump_dom",
                "target": public_target,
                "exit_code": exit_code,
                "stdout_total_bytes": execution.stdout.total_bytes,
                "stdout_truncated": execution.stdout.truncated,
                "stderr_total_bytes": execution.stderr.total_bytes,
                "stderr_truncated": execution.stderr.truncated,
            }),
            provenance: Provenance {
                source: "chromium-compatibility".into(),
                engine: Some("chromium".into()),
                revision: revision.map(str::to_owned),
                parent_ids: Vec::new(),
                captured_at,
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        })
        .await;

    Ok(ChromiumCompatibilityReceipt {
        target: public_target,
        exit_code,
        stdout_total_bytes: execution.stdout.total_bytes,
        stdout_truncated: execution.stdout.truncated,
        stderr_total_bytes: execution.stderr.total_bytes,
        stderr_truncated: execution.stderr.truncated,
        evidence_id: evidence.id,
    })
}

pub(crate) fn canonical_chromium_route_identity(base: &Url, route: &str) -> Option<String> {
    resolve_route_target(base, route).map(|target| private_safe_route_identity(&target))
}

fn private_safe_route_identity(target: &Url) -> String {
    let mut identity = target.clone();
    identity.set_query(None);
    identity.set_fragment(None);
    identity.to_string()
}

fn config(state: &ControlState) -> Option<ChromiumExecutorConfig> {
    let key = Arc::as_ptr(&state.sessions) as usize;
    let registry = CHROMIUM_EXECUTORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get(&key).map(|entry| entry.config.clone())
}

async fn resolve_target(
    state: &ControlState,
    id: SessionId,
) -> Result<Url, ChromiumRuntimeError> {
    let session = state
        .sessions
        .get(id)
        .await
        .ok_or(ChromiumRuntimeError::SessionNotFound)?;
    let base = session
        .endpoint
        .url()
        .map_err(|_| ChromiumRuntimeError::InvalidTarget)?;
    if !validate_loopback_url(&base) {
        return Err(ChromiumRuntimeError::InvalidTarget);
    }

    let observed_route = state
        .live
        .recent(id, 2048)
        .await
        .into_iter()
        .rev()
        .find_map(|event| event.route);
    match observed_route {
        Some(route) => resolve_route_target(&base, &route).ok_or(ChromiumRuntimeError::InvalidTarget),
        None => Ok(base),
    }
}

fn resolve_route_target(base: &Url, route: &str) -> Option<Url> {
    if !validate_loopback_url(base) {
        return None;
    }
    let target = Url::parse(route).or_else(|_| base.join(route)).ok()?;
    if !validate_loopback_url(&target) || target.origin() != base.origin() {
        return None;
    }
    Some(target)
}

fn lock_registry(registry: &Mutex<ChromiumRegistry>) -> MutexGuard<'_, ChromiumRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
