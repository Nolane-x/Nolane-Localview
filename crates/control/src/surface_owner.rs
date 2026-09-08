#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
    time::{Duration, Instant},
};

use localview_sessions::SessionManager;
use serde::Serialize;
use uuid::Uuid;

use crate::surface_recovery::surface_recovery_journal_for_sessions;

pub(crate) const SURFACE_OWNER_TTL: Duration = Duration::from_secs(15);

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
pub(crate) struct SurfaceOwnerOperationGuard {
    sessions: Arc<SessionManager>,
    owner_instance_id: Uuid,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceOwnerLease {
    owner_lease_id: Uuid,
    last_seen: Instant,
    in_flight: usize,
}

#[derive(Debug)]
struct SurfaceOwnerEntry {
    owner: Weak<SessionManager>,
    boot_epoch: Uuid,
    leases: BTreeMap<Uuid, SurfaceOwnerLease>,
}

type SurfaceOwnerRegistry = HashMap<usize, SurfaceOwnerEntry>;

static SURFACE_OWNERS: OnceLock<Mutex<SurfaceOwnerRegistry>> = OnceLock::new();

pub(crate) fn register_surface_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    owner_instance_id: Uuid,
) -> SurfaceOwnerRegistration {
    register_surface_owner_for_sessions_at(sessions, owner_instance_id, Instant::now())
}

fn register_surface_owner_for_sessions_at(
    sessions: &Arc<SessionManager>,
    owner_instance_id: Uuid,
    now: Instant,
) -> SurfaceOwnerRegistration {
    let recovery_required = surface_recovery_journal_for_sessions(sessions)
        .is_some_and(|journal| journal.has_outstanding_for_owner(owner_instance_id));

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
    let in_flight = entry
        .leases
        .get(&owner_instance_id)
        .map_or(0, |lease| lease.in_flight);
    entry.leases.insert(
        owner_instance_id,
        SurfaceOwnerLease {
            owner_lease_id,
            last_seen: now,
            in_flight,
        },
    );

    SurfaceOwnerRegistration {
        owner_instance_id,
        boot_epoch: entry.boot_epoch,
        owner_lease_id,
        recovery_required,
    }
}

pub(crate) fn validate_surface_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
) -> Result<(), SurfaceOwnerError> {
    validate_surface_owner_for_sessions_at(sessions, proof, Instant::now())
}

fn validate_surface_owner_for_sessions_at(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
    now: Instant,
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
    let Some(current) = entry.leases.get(&proof.owner_instance_id) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.owner_lease_id != current.owner_lease_id {
        return Err(SurfaceOwnerError::LeaseMismatch);
    }
    if owner_expired(current.last_seen, now) {
        return Err(SurfaceOwnerError::NotRegistered);
    }
    Ok(())
}

pub(crate) fn pin_surface_owner_for_sessions(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
) -> Result<SurfaceOwnerOperationGuard, SurfaceOwnerError> {
    pin_surface_owner_for_sessions_at(sessions, proof, Instant::now())
}

fn pin_surface_owner_for_sessions_at(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
    now: Instant,
) -> Result<SurfaceOwnerOperationGuard, SurfaceOwnerError> {
    let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    let key = Arc::as_ptr(sessions) as usize;
    let Some(entry) = entries.get_mut(&key) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.boot_epoch != entry.boot_epoch {
        return Err(SurfaceOwnerError::BootEpochMismatch);
    }
    let Some(current) = entry.leases.get_mut(&proof.owner_instance_id) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.owner_lease_id != current.owner_lease_id {
        return Err(SurfaceOwnerError::LeaseMismatch);
    }
    if owner_expired(current.last_seen, now) {
        return Err(SurfaceOwnerError::NotRegistered);
    }
    current.in_flight = current.in_flight.saturating_add(1);

    Ok(SurfaceOwnerOperationGuard {
        sessions: sessions.clone(),
        owner_instance_id: proof.owner_instance_id,
    })
}

impl Drop for SurfaceOwnerOperationGuard {
    fn drop(&mut self) {
        let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut entries = lock_registry(registry);
        entries.retain(|_, entry| entry.owner.strong_count() > 0);

        let key = Arc::as_ptr(&self.sessions) as usize;
        if let Some(entry) = entries.get_mut(&key) {
            if let Some(current) = entry.leases.get_mut(&self.owner_instance_id) {
                current.in_flight = current.in_flight.saturating_sub(1);
            }
        }
    }
}

pub(crate) fn heartbeat_surface_owner_for_sessions_at(
    sessions: &Arc<SessionManager>,
    proof: SurfaceOwnerProof,
    now: Instant,
) -> Result<(), SurfaceOwnerError> {
    let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    let key = Arc::as_ptr(sessions) as usize;
    let Some(entry) = entries.get_mut(&key) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.boot_epoch != entry.boot_epoch {
        return Err(SurfaceOwnerError::BootEpochMismatch);
    }
    let Some(current) = entry.leases.get_mut(&proof.owner_instance_id) else {
        return Err(SurfaceOwnerError::NotRegistered);
    };
    if proof.owner_lease_id != current.owner_lease_id {
        return Err(SurfaceOwnerError::LeaseMismatch);
    }
    if owner_expired(current.last_seen, now) {
        return Err(SurfaceOwnerError::NotRegistered);
    }
    current.last_seen = now;
    Ok(())
}

pub(crate) fn reap_expired_surface_owners_for_sessions_at(
    sessions: &Arc<SessionManager>,
    now: Instant,
) -> Vec<Uuid> {
    let registry = SURFACE_OWNERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_registry(registry);
    entries.retain(|_, entry| entry.owner.strong_count() > 0);

    let key = Arc::as_ptr(sessions) as usize;
    let Some(entry) = entries.get_mut(&key) else {
        return Vec::new();
    };

    let mut expired = Vec::new();
    entry.leases.retain(|owner_instance_id, lease| {
        if owner_expired(lease.last_seen, now) && lease.in_flight == 0 {
            expired.push(*owner_instance_id);
            false
        } else {
            true
        }
    });
    expired
}

fn owner_expired(last_seen: Instant, now: Instant) -> bool {
    now.checked_duration_since(last_seen)
        .is_some_and(|elapsed| elapsed >= SURFACE_OWNER_TTL)
}

fn lock_registry(registry: &Mutex<SurfaceOwnerRegistry>) -> MutexGuard<'_, SurfaceOwnerRegistry> {
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_operation_delays_reap_without_authorizing_new_expired_requests() {
        let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
        let owner_instance_id = Uuid::new_v4();
        let start = Instant::now();
        let registration =
            register_surface_owner_for_sessions_at(&sessions, owner_instance_id, start);
        let proof = SurfaceOwnerProof {
            owner_instance_id,
            boot_epoch: registration.boot_epoch,
            owner_lease_id: registration.owner_lease_id,
        };

        let guard = pin_surface_owner_for_sessions_at(
            &sessions,
            proof,
            start + Duration::from_secs(1),
        )
        .expect("current owner operation must acquire a liveness pin");
        let expired_at = start + Duration::from_secs(16);

        assert_eq!(
            validate_surface_owner_for_sessions_at(&sessions, proof, expired_at),
            Err(SurfaceOwnerError::NotRegistered),
            "an in-flight pin must never authorize a new request after TTL expiry"
        );
        assert!(
            reap_expired_surface_owners_for_sessions_at(&sessions, expired_at).is_empty(),
            "reaper must not revoke an owner while a previously-authorized operation is committing"
        );

        drop(guard);
        assert_eq!(
            reap_expired_surface_owners_for_sessions_at(&sessions, expired_at),
            vec![owner_instance_id],
            "owner must become reapable immediately after the in-flight operation finishes"
        );
    }
}
