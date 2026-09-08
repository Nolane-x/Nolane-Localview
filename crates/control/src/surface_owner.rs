#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use localview_sessions::SessionManager;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceOwnerProof {
    pub owner_instance_id: Uuid,
    pub boot_epoch: Uuid,
    pub owner_lease_id: Uuid,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(crate) struct SurfaceOwnerRegistration {
    pub owner_instance_id: Uuid,
    pub boot_epoch: Uuid,
    pub owner_lease_id: Uuid,
    pub recovery_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceOwnerError {
    NotRegistered,
    BootEpochMismatch,
    LeaseMismatch,
}

#[derive(Debug)]
struct SurfaceOwnerEntry {
    owner: Weak<SessionManager>,
    boot_epoch: Uuid,
    leases: BTreeMap<Uuid, Uuid>,
}

type SurfaceOwnerRegistry = HashMap<usize, SurfaceOwnerEntry>;

static SURFACE_OWNERS: OnceLock<Mutex<SurfaceOwnerRegistry>> = OnceLock::new();

pub(crate) fn register_surface_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    owner_instance_id: Uuid,
) -> SurfaceOwnerRegistration {
    let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    let key = Arc::as_ptr(sessions) as usize;
    let entry = entries.entry(key).or_insert_with(|| SurfaceOwnerEntry {
        owner: Arc::downgrade(sessions),
        boot_epoch: Uuid::new_v4(),
        leases: BTreeMap::new(),
    });
    let owner_lease_id = Uuid::new_v4();
    entry.leases.insert(owner_instance_id, owner_lease_id);

    SurfaceOwnerRegistration {
        owner_instance_id,
        boot_epoch: entry.boot_epoch,
        owner_lease_id,
        recovery_required: false,
    }
}

pub(crate) fn validate_surface_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
) -> Result<(), SurfaceOwnerError> {
    let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    let key = Arc::as_ptr(sessions) as usize;
    let Some(entry) = entries.get(&key) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.boot_epoch != entry.boot_epoch {
        return Err(SurfaceOwnerError::BootEpochMismatch);
    }
    let Some(current_lease) = entry.leases.get(&proof.owner_instance_id) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.owner_lease_id != *current_lease {
        return Err(SurfaceOwnerError::LeaseMismatch);
    }
    Ok(())
}

fn lock_registry(registry: &Mutex<SurfaceOwnerRegistry>) -> MutexGuard<'_, SurfaceOwnerRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
