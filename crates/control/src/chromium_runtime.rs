#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::Duration,
};

use localview_chromium::ChromiumExecutionPolicy;
use localview_sessions::SessionManager;

use crate::ControlState;

const DEFAULT_CHROMIUM_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_CHROMIUM_STDOUT_BYTES: usize = 64 * 1024;
const DEFAULT_CHROMIUM_STDERR_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct ChromiumExecutorConfig {
    pub(crate) executable: PathBuf,
    pub(crate) policy: ChromiumExecutionPolicy,
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
}

pub(crate) fn config(state: &ControlState) -> Option<ChromiumExecutorConfig> {
    let key = Arc::as_ptr(&state.sessions) as usize;
    let registry = CHROMIUM_EXECUTORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries.get(&key).map(|entry| entry.config.clone())
}

fn lock_registry(registry: &Mutex<ChromiumRegistry>) -> MutexGuard<'_, ChromiumRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
