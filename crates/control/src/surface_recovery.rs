#![forbid(unsafe_code)]

use std::{
    collections::{BTreeSet, HashMap},
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use localview_protocol::SessionId;
use localview_sessions::SessionManager;
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    sync::Mutex as AsyncMutex,
};
use uuid::Uuid;

pub const SURFACE_RECOVERY_JOURNAL_FILE: &str = "surface-recovery.jsonl";

const MAX_EVENT_BYTES: usize = 2 * 1024;
const MAX_JOURNAL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_JOURNAL_EVENTS: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRecoveryKey {
    pub session_id: SessionId,
    pub surface_kind: String,
    pub label: String,
    pub incarnation: u64,
    pub owner_instance_id: Uuid,
}

impl SurfaceRecoveryKey {
    pub fn new(
        session_id: SessionId,
        surface_kind: impl Into<String>,
        label: impl Into<String>,
        incarnation: u64,
        owner_instance_id: Uuid,
    ) -> Result<Self, SurfaceRecoveryError> {
        let key = Self {
            session_id,
            surface_kind: surface_kind.into(),
            label: label.into(),
            incarnation,
            owner_instance_id,
        };
        key.validate()?;
        Ok(key)
    }

    fn validate(&self) -> Result<(), SurfaceRecoveryError> {
        if self.session_id.is_nil()
            || self.owner_instance_id.is_nil()
            || self.incarnation == 0
            || !matches!(
                self.surface_kind.as_str(),
                "preview_window" | "workspace_child"
            )
            || self.label.is_empty()
            || self.label.len() > 160
            || self.label.chars().any(char::is_control)
        {
            return Err(SurfaceRecoveryError::InvalidKey);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum SurfaceRecoveryEvent {
    Activated { key: SurfaceRecoveryKey },
    Released { key: SurfaceRecoveryKey },
    Reattached { key: SurfaceRecoveryKey },
}

impl SurfaceRecoveryEvent {
    fn key(&self) -> &SurfaceRecoveryKey {
        match self {
            Self::Activated { key } | Self::Released { key } | Self::Reattached { key } => key,
        }
    }

    fn apply(self, outstanding: &mut BTreeSet<SurfaceRecoveryKey>) {
        match self {
            Self::Activated { key } => {
                outstanding.insert(key);
            }
            Self::Released { key } | Self::Reattached { key } => {
                outstanding.remove(&key);
            }
        }
    }
}

#[derive(Debug)]
pub enum SurfaceRecoveryError {
    Io(std::io::Error),
    CorruptJournal,
    InvalidKey,
    EventTooLarge,
    JournalFull,
}

impl fmt::Display for SurfaceRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "surface recovery journal I/O failed: {error}"),
            Self::CorruptJournal => f.write_str("surface recovery journal is corrupt"),
            Self::InvalidKey => f.write_str("surface recovery key is invalid"),
            Self::EventTooLarge => f.write_str("surface recovery event exceeds bounded size"),
            Self::JournalFull => f.write_str("surface recovery journal reached its bounded limit"),
        }
    }
}

impl Error for SurfaceRecoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SurfaceRecoveryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
struct SurfaceRecoveryState {
    outstanding: BTreeSet<SurfaceRecoveryKey>,
    event_count: usize,
    byte_count: u64,
}

#[derive(Debug)]
struct SurfaceRecoveryInner {
    path: PathBuf,
    state: Mutex<SurfaceRecoveryState>,
    append_lock: AsyncMutex<()>,
}

#[derive(Debug, Clone)]
pub struct SurfaceRecoveryJournal {
    inner: Arc<SurfaceRecoveryInner>,
}

impl SurfaceRecoveryJournal {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, SurfaceRecoveryError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }

        let bytes = match fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(SurfaceRecoveryError::JournalFull);
        }

        let text = std::str::from_utf8(&bytes).map_err(|_| SurfaceRecoveryError::CorruptJournal)?;
        let mut outstanding = BTreeSet::new();
        let mut event_count = 0usize;
        for line in text.lines() {
            if line.is_empty() || line.len() > MAX_EVENT_BYTES {
                return Err(SurfaceRecoveryError::CorruptJournal);
            }
            event_count = event_count
                .checked_add(1)
                .ok_or(SurfaceRecoveryError::JournalFull)?;
            if event_count > MAX_JOURNAL_EVENTS {
                return Err(SurfaceRecoveryError::JournalFull);
            }
            let event: SurfaceRecoveryEvent =
                serde_json::from_str(line).map_err(|_| SurfaceRecoveryError::CorruptJournal)?;
            event.key().validate()?;
            event.apply(&mut outstanding);
        }

        Ok(Self {
            inner: Arc::new(SurfaceRecoveryInner {
                path,
                state: Mutex::new(SurfaceRecoveryState {
                    outstanding,
                    event_count,
                    byte_count: bytes.len() as u64,
                }),
                append_lock: AsyncMutex::new(()),
            }),
        })
    }

    pub async fn record_activated(
        &self,
        key: SurfaceRecoveryKey,
    ) -> Result<(), SurfaceRecoveryError> {
        self.record(SurfaceRecoveryEvent::Activated { key }).await
    }

    pub async fn record_released(
        &self,
        key: SurfaceRecoveryKey,
    ) -> Result<(), SurfaceRecoveryError> {
        self.record(SurfaceRecoveryEvent::Released { key }).await
    }

    pub async fn record_reattached(
        &self,
        key: SurfaceRecoveryKey,
    ) -> Result<(), SurfaceRecoveryError> {
        self.record(SurfaceRecoveryEvent::Reattached { key }).await
    }

    pub fn outstanding_exact(&self, key: &SurfaceRecoveryKey) -> bool {
        lock_state(&self.inner.state).outstanding.contains(key)
    }

    pub fn has_outstanding_for_owner(&self, owner_instance_id: Uuid) -> bool {
        lock_state(&self.inner.state)
            .outstanding
            .iter()
            .any(|key| key.owner_instance_id == owner_instance_id)
    }

    pub fn outstanding_len(&self) -> usize {
        lock_state(&self.inner.state).outstanding.len()
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    async fn record(&self, event: SurfaceRecoveryEvent) -> Result<(), SurfaceRecoveryError> {
        event.key().validate()?;
        let mut encoded =
            serde_json::to_vec(&event).map_err(|_| SurfaceRecoveryError::CorruptJournal)?;
        encoded.push(b'\n');
        if encoded.len() > MAX_EVENT_BYTES {
            return Err(SurfaceRecoveryError::EventTooLarge);
        }

        let _append_guard = self.inner.append_lock.lock().await;
        {
            let state = lock_state(&self.inner.state);
            if state.event_count >= MAX_JOURNAL_EVENTS
                || state.byte_count.saturating_add(encoded.len() as u64) > MAX_JOURNAL_BYTES
            {
                return Err(SurfaceRecoveryError::JournalFull);
            }
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.inner.path)
            .await?;
        file.write_all(&encoded).await?;
        file.flush().await?;
        file.sync_data().await?;

        let mut state = lock_state(&self.inner.state);
        state.event_count += 1;
        state.byte_count += encoded.len() as u64;
        event.apply(&mut state.outstanding);
        Ok(())
    }
}

#[derive(Debug)]
struct SurfaceRecoveryRegistryEntry {
    owner: Weak<SessionManager>,
    journal: Arc<SurfaceRecoveryJournal>,
}

type SurfaceRecoveryRegistry = HashMap<usize, SurfaceRecoveryRegistryEntry>;

static SURFACE_RECOVERY_JOURNALS: OnceLock<Mutex<SurfaceRecoveryRegistry>> = OnceLock::new();

pub fn configure_surface_recovery_journal_for_sessions(
    sessions: &Arc<SessionManager>,
    journal: Option<Arc<SurfaceRecoveryJournal>>,
) {
    let registry = SURFACE_RECOVERY_JOURNALS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    let key = Arc::as_ptr(sessions) as usize;
    match journal {
        Some(journal) => {
            entries.insert(
                key,
                SurfaceRecoveryRegistryEntry {
                    owner: Arc::downgrade(sessions),
                    journal,
                },
            );
        }
        None => {
            entries.remove(&key);
        }
    }
}

pub(crate) fn surface_recovery_journal_for_sessions(
    sessions: &Arc<SessionManager>,
) -> Option<Arc<SurfaceRecoveryJournal>> {
    let registry = SURFACE_RECOVERY_JOURNALS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);
    entries
        .get(&(Arc::as_ptr(sessions) as usize))
        .map(|entry| entry.journal.clone())
}

fn lock_state(state: &Mutex<SurfaceRecoveryState>) -> MutexGuard<'_, SurfaceRecoveryState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_registry(
    registry: &Mutex<SurfaceRecoveryRegistry>,
) -> MutexGuard<'_, SurfaceRecoveryRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
