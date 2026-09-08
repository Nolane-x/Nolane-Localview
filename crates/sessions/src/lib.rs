#![forbid(unsafe_code)]

mod identity;

pub use identity::{
    MAX_ENDPOINT_HOST_BYTES, MAX_ENDPOINT_SCHEME_BYTES, MAX_NORMALIZED_PROJECT_PATH_BYTES,
    MAX_SESSION_IDENTITY_RECORDS, MAX_SESSION_IDENTITY_REGISTRY_BYTES,
    SESSION_IDENTITY_REGISTRY_FILE, ResolvedSessionIdentity, SessionIdentityDurability,
    SessionIdentityError, SessionIdentityHealth, SessionIdentityResolver, SessionLineage,
    SessionLineageAnchorV1, SessionLineageV1, SessionServerKind, session_lineage,
};

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Duration,
};

use chrono::{DateTime, Utc};
use localview_core::project_identity;
use localview_protocol::{
    DiscoveredServer, Endpoint, ProjectIdentity, Session, SessionId, SessionStatus,
};
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use uuid::Uuid;

#[derive(Debug, Default)]
struct SessionState {
    sessions: HashMap<SessionId, Session>,
    lineages: HashMap<SessionId, SessionLineage>,
}

#[derive(Debug)]
pub struct SessionManager {
    state: RwLock<SessionState>,
    grace: Duration,
    identity_resolver: Option<SessionIdentityResolver>,
    reconcile_gate: AsyncMutex<()>,
}

#[derive(Debug, Default)]
pub struct ReconcileResult {
    pub created: Vec<SessionId>,
    pub reconnected: Vec<SessionId>,
    pub disconnected: Vec<SessionId>,
    pub removed: Vec<SessionId>,
}

#[derive(Debug)]
struct PreparedDiscovery {
    server: DiscoveredServer,
    project: ProjectIdentity,
    lineage: Option<SessionLineage>,
    ambiguous: bool,
}

impl SessionManager {
    pub fn new(grace: Duration) -> Self {
        Self {
            state: RwLock::new(SessionState::default()),
            grace,
            identity_resolver: None,
            reconcile_gate: AsyncMutex::new(()),
        }
    }

    pub fn with_identity_resolver(
        grace: Duration,
        identity_resolver: SessionIdentityResolver,
    ) -> Self {
        Self {
            state: RwLock::new(SessionState::default()),
            grace,
            identity_resolver: Some(identity_resolver),
            reconcile_gate: AsyncMutex::new(()),
        }
    }

    pub async fn list(&self) -> Vec<Session> {
        let state = self.state.read().await;
        let mut sessions = state.sessions.values().cloned().collect::<Vec<_>>();
        sessions.sort_by_key(|session| {
            (session.project.display_name.clone(), session.endpoint.port)
        });
        sessions
    }

    pub async fn get(&self, id: SessionId) -> Option<Session> {
        self.state.read().await.sessions.get(&id).cloned()
    }

    pub async fn set_preview_visible(&self, id: SessionId, visible: bool) -> bool {
        let mut state = self.state.write().await;
        let Some(session) = state.sessions.get_mut(&id) else {
            return false;
        };
        session.preview_visible = visible;
        session.status = if visible || session.status == SessionStatus::Active {
            SessionStatus::Active
        } else {
            SessionStatus::Hidden
        };
        true
    }

    pub async fn reconcile(
        &self,
        discovered: Vec<DiscoveredServer>,
        now: DateTime<Utc>,
    ) -> ReconcileResult {
        let _reconcile_guard = self.reconcile_gate.lock().await;
        let prepared = prepare_discovery_batch(discovered);

        let snapshot = {
            let state = self.state.read().await;
            SessionState {
                sessions: state.sessions.clone(),
                lineages: state.lineages.clone(),
            }
        };

        let assignments = self.resolve_assignments(&prepared, &snapshot).await;
        self.apply_reconciliation(prepared, assignments, now).await
    }

    async fn resolve_assignments(
        &self,
        prepared: &[PreparedDiscovery],
        snapshot: &SessionState,
    ) -> Vec<SessionId> {
        let mut assigned = HashSet::new();
        let mut assignments = Vec::with_capacity(prepared.len());

        for item in prepared {
            let current = if item.ambiguous {
                find_exact_endpoint_match(snapshot, &item.server.candidate.endpoint, &assigned)
            } else {
                item.lineage
                    .as_ref()
                    .and_then(|lineage| find_lineage_match(snapshot, lineage, &assigned))
                    .or_else(|| find_legacy_project_match(snapshot, item, &assigned))
                    .or_else(|| {
                        find_exact_endpoint_match(
                            snapshot,
                            &item.server.candidate.endpoint,
                            &assigned,
                        )
                    })
            };

            let session_id = if let Some(session_id) = current {
                session_id
            } else if item.ambiguous {
                fresh_volatile_session_id(snapshot, &assigned)
            } else if let (Some(resolver), Some(lineage)) =
                (self.identity_resolver.as_ref(), item.lineage.as_ref())
            {
                let resolved = resolver.resolve_new(lineage).await;
                if id_available_for_lineage(snapshot, &assigned, resolved.session_id, lineage) {
                    resolved.session_id
                } else {
                    fresh_volatile_session_id(snapshot, &assigned)
                }
            } else {
                fresh_volatile_session_id(snapshot, &assigned)
            };

            assigned.insert(session_id);
            assignments.push(session_id);
        }

        assignments
    }

    async fn apply_reconciliation(
        &self,
        prepared: Vec<PreparedDiscovery>,
        assignments: Vec<SessionId>,
        now: DateTime<Utc>,
    ) -> ReconcileResult {
        let mut state = self.state.write().await;
        let mut result = ReconcileResult::default();
        let mut seen = HashSet::new();

        for (item, session_id) in prepared.into_iter().zip(assignments) {
            seen.insert(session_id);
            if let Some(session) = state.sessions.get_mut(&session_id) {
                if matches!(
                    session.status,
                    SessionStatus::Disconnected | SessionStatus::Hidden
                ) && session.disconnected_at.is_some()
                {
                    result.reconnected.push(session_id);
                }
                session.endpoint = item.server.candidate.endpoint;
                session.classification = item.server.classification;
                session.project = item.project;
                session.status = if session.preview_visible {
                    SessionStatus::Active
                } else {
                    SessionStatus::Hidden
                };
                session.last_seen = now;
                session.disconnected_at = None;
            } else {
                result.created.push(session_id);
                state.sessions.insert(
                    session_id,
                    Session {
                        id: session_id,
                        endpoint: item.server.candidate.endpoint,
                        classification: item.server.classification,
                        project: item.project,
                        status: SessionStatus::Active,
                        first_seen: now,
                        last_seen: now,
                        disconnected_at: None,
                        preview_visible: false,
                    },
                );
            }

            match item.lineage {
                Some(lineage) => {
                    state.lineages.insert(session_id, lineage);
                }
                None => {
                    state.lineages.remove(&session_id);
                }
            }
        }

        let grace = chrono::Duration::from_std(self.grace)
            .unwrap_or_else(|_| chrono::Duration::seconds(3));
        let ids = state.sessions.keys().copied().collect::<Vec<_>>();
        for id in ids {
            if seen.contains(&id) {
                continue;
            }
            let mut should_remove = false;
            if let Some(session) = state.sessions.get_mut(&id) {
                match session.disconnected_at.as_ref() {
                    None => {
                        session.disconnected_at = Some(now);
                        session.status = SessionStatus::Disconnected;
                        result.disconnected.push(id);
                    }
                    Some(disconnected_at) => {
                        should_remove = now.signed_duration_since(disconnected_at.to_owned()) >= grace;
                    }
                }
            }
            if should_remove {
                state.sessions.remove(&id);
                state.lineages.remove(&id);
                result.removed.push(id);
            }
        }

        result
    }
}

fn prepare_discovery_batch(discovered: Vec<DiscoveredServer>) -> Vec<PreparedDiscovery> {
    let mut prepared = discovered
        .into_iter()
        .map(|server| {
            let project = project_identity(&server.candidate);
            let lineage = session_lineage(
                &project,
                &server.candidate.endpoint,
                server.classification.kind,
            )
            .ok();
            PreparedDiscovery {
                server,
                project,
                lineage,
                ambiguous: false,
            }
        })
        .collect::<Vec<_>>();

    let mut lineage_counts = BTreeMap::<SessionLineage, usize>::new();
    for item in &prepared {
        if let Some(lineage) = &item.lineage {
            *lineage_counts.entry(lineage.clone()).or_default() += 1;
        }
    }
    for item in &mut prepared {
        item.ambiguous = item
            .lineage
            .as_ref()
            .and_then(|lineage| lineage_counts.get(lineage))
            .is_some_and(|count| *count > 1);
    }

    prepared
}

fn find_lineage_match(
    state: &SessionState,
    lineage: &SessionLineage,
    assigned: &HashSet<SessionId>,
) -> Option<SessionId> {
    state.lineages.iter().find_map(|(session_id, current)| {
        (*current == *lineage && !assigned.contains(session_id)).then_some(*session_id)
    })
}

fn find_legacy_project_match(
    state: &SessionState,
    item: &PreparedDiscovery,
    assigned: &HashSet<SessionId>,
) -> Option<SessionId> {
    state.sessions.values().find_map(|session| {
        (session.project.key == item.project.key
            && session.classification.kind == item.server.classification.kind
            && !assigned.contains(&session.id))
        .then_some(session.id)
    })
}

fn find_exact_endpoint_match(
    state: &SessionState,
    endpoint: &Endpoint,
    assigned: &HashSet<SessionId>,
) -> Option<SessionId> {
    state.sessions.values().find_map(|session| {
        (same_endpoint(&session.endpoint, endpoint) && !assigned.contains(&session.id))
            .then_some(session.id)
    })
}

fn id_available_for_lineage(
    state: &SessionState,
    assigned: &HashSet<SessionId>,
    session_id: SessionId,
    lineage: &SessionLineage,
) -> bool {
    if assigned.contains(&session_id) {
        return false;
    }
    match state.sessions.get(&session_id) {
        None => true,
        Some(_) => state.lineages.get(&session_id) == Some(lineage),
    }
}

fn fresh_volatile_session_id(
    state: &SessionState,
    assigned: &HashSet<SessionId>,
) -> SessionId {
    loop {
        let candidate = Uuid::new_v4();
        if candidate != Uuid::nil()
            && !assigned.contains(&candidate)
            && !state.sessions.contains_key(&candidate)
        {
            return candidate;
        }
    }
}

fn same_endpoint(a: &Endpoint, b: &Endpoint) -> bool {
    a.host == b.host && a.port == b.port && a.scheme == b.scheme
}

#[cfg(test)]
mod tests {
    use super::*;
    use localview_protocol::{Classification, ListenerCandidate, ServerKind};

    fn discovered(port: u16) -> DiscoveredServer {
        DiscoveredServer {
            candidate: ListenerCandidate {
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port,
                    scheme: "http".into(),
                },
                pid: Some(9),
                process_name: None,
                command: Some("vite".into()),
                cwd: Some("/tmp/app".into()),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("Vite".into()),
                title: None,
                hmr_detected: true,
                evidence: Default::default(),
            },
        }
    }

    #[tokio::test]
    async fn reconnects_same_project_when_port_changes() {
        let manager = SessionManager::new(Duration::from_secs(2));
        let t = Utc::now();
        let first = manager.reconcile(vec![discovered(5173)], t).await;
        let id = first.created[0];
        let moved = manager
            .reconcile(
                vec![discovered(5174)],
                t + chrono::Duration::milliseconds(500),
            )
            .await;
        assert!(moved.created.is_empty());
        assert_eq!(
            manager.get(id).await.expect("session should exist").endpoint.port,
            5174
        );
    }

    #[tokio::test]
    async fn removes_after_grace() {
        let manager = SessionManager::new(Duration::from_secs(1));
        let t = Utc::now();
        manager.reconcile(vec![discovered(5173)], t).await;
        assert_eq!(
            manager
                .reconcile(vec![], t + chrono::Duration::milliseconds(10))
                .await
                .disconnected
                .len(),
            1
        );
        assert_eq!(
            manager
                .reconcile(vec![], t + chrono::Duration::seconds(2))
                .await
                .removed
                .len(),
            1
        );
    }
}
