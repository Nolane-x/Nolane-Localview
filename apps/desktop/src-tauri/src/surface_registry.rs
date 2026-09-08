#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard},
};

use localview_protocol::SessionId;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DesktopSurfaceKind {
    PreviewWindow,
    WorkspaceChild,
}

impl DesktopSurfaceKind {
    pub const fn as_runtime_kind(self) -> &'static str {
        match self {
            Self::PreviewWindow => "preview_window",
            Self::WorkspaceChild => "workspace_child",
        }
    }

    pub fn from_runtime_kind(value: &str) -> Option<Self> {
        match value {
            "preview_window" => Some(Self::PreviewWindow),
            "workspace_child" => Some(Self::WorkspaceChild),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSurfaceVisibility {
    Visible,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSurfaceIdentity {
    pub session_id: SessionId,
    pub kind: DesktopSurfaceKind,
    pub label: String,
    pub incarnation: u64,
    pub owner_instance_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSurfaceSnapshot {
    pub identity: DesktopSurfaceIdentity,
    pub visibility: DesktopSurfaceVisibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSurfaceRegistryError {
    AlreadyLive,
    IncarnationMismatch,
    NotLive,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DesktopSurfaceKey {
    session_id: String,
    kind: DesktopSurfaceKind,
    label: String,
}

impl DesktopSurfaceKey {
    fn new(session_id: SessionId, kind: DesktopSurfaceKind, label: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            kind,
            label: label.to_owned(),
        }
    }

    fn from_identity(identity: &DesktopSurfaceIdentity) -> Self {
        Self::new(identity.session_id, identity.kind, &identity.label)
    }
}

#[derive(Debug, Default)]
struct RegistryState {
    incarnations: BTreeMap<DesktopSurfaceKey, u64>,
    live: BTreeMap<DesktopSurfaceKey, DesktopSurfaceSnapshot>,
}

#[derive(Debug)]
pub struct DesktopSurfaceRegistry {
    owner_instance_id: Uuid,
    inner: Mutex<RegistryState>,
}

impl Default for DesktopSurfaceRegistry {
    fn default() -> Self {
        Self {
            owner_instance_id: Uuid::new_v4(),
            inner: Mutex::new(RegistryState::default()),
        }
    }
}

impl DesktopSurfaceRegistry {
    pub fn owner_instance_id(&self) -> Uuid {
        self.owner_instance_id
    }

    pub fn next_identity(
        &self,
        session_id: SessionId,
        kind: DesktopSurfaceKind,
        label: impl Into<String>,
    ) -> DesktopSurfaceIdentity {
        let label = label.into();
        let key = DesktopSurfaceKey::new(session_id, kind, &label);
        let mut state = self.lock();
        let incarnation = state
            .incarnations
            .entry(key)
            .and_modify(|current| {
                *current = current
                    .checked_add(1)
                    .expect("desktop surface incarnation space exhausted");
            })
            .or_insert(1);

        DesktopSurfaceIdentity {
            session_id,
            kind,
            label,
            incarnation: *incarnation,
            owner_instance_id: self.owner_instance_id,
        }
    }

    pub fn record_created(
        &self,
        identity: DesktopSurfaceIdentity,
        visibility: DesktopSurfaceVisibility,
    ) -> Result<(), DesktopSurfaceRegistryError> {
        if identity.owner_instance_id != self.owner_instance_id {
            return Err(DesktopSurfaceRegistryError::IncarnationMismatch);
        }
        let key = DesktopSurfaceKey::from_identity(&identity);
        let mut state = self.lock();

        if let Some(current) = state.live.get(&key) {
            return if current.identity == identity {
                Err(DesktopSurfaceRegistryError::AlreadyLive)
            } else {
                Err(DesktopSurfaceRegistryError::IncarnationMismatch)
            };
        }

        if state.incarnations.get(&key).copied() != Some(identity.incarnation) {
            return Err(DesktopSurfaceRegistryError::IncarnationMismatch);
        }

        state.live.insert(
            key,
            DesktopSurfaceSnapshot {
                identity,
                visibility,
            },
        );
        Ok(())
    }

    pub fn set_visibility(
        &self,
        identity: &DesktopSurfaceIdentity,
        visibility: DesktopSurfaceVisibility,
    ) -> Result<(), DesktopSurfaceRegistryError> {
        let key = DesktopSurfaceKey::from_identity(identity);
        let mut state = self.lock();
        let Some(current) = state.live.get_mut(&key) else {
            return Err(DesktopSurfaceRegistryError::NotLive);
        };
        if current.identity != *identity {
            return Err(DesktopSurfaceRegistryError::IncarnationMismatch);
        }
        current.visibility = visibility;
        Ok(())
    }

    pub fn record_closed(
        &self,
        identity: &DesktopSurfaceIdentity,
    ) -> Result<(), DesktopSurfaceRegistryError> {
        let key = DesktopSurfaceKey::from_identity(identity);
        let mut state = self.lock();
        let Some(current) = state.live.get(&key) else {
            return Err(DesktopSurfaceRegistryError::NotLive);
        };
        if current.identity != *identity {
            return Err(DesktopSurfaceRegistryError::IncarnationMismatch);
        }
        state.live.remove(&key);
        Ok(())
    }

    pub fn current(
        &self,
        session_id: SessionId,
        kind: DesktopSurfaceKind,
        label: &str,
    ) -> Option<DesktopSurfaceSnapshot> {
        self.lock()
            .live
            .get(&DesktopSurfaceKey::new(session_id, kind, label))
            .cloned()
    }

    pub fn live_count(&self) -> usize {
        self.lock().live.len()
    }

    fn lock(&self) -> MutexGuard<'_, RegistryState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
