#![forbid(unsafe_code)]

use std::{collections::{HashMap, HashSet}, time::Duration};
use chrono::{DateTime, Utc};
use localview_core::project_identity;
use localview_protocol::{DiscoveredServer, Endpoint, Session, SessionId, SessionStatus};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug)]
pub struct SessionManager { sessions: RwLock<HashMap<SessionId, Session>>, grace: Duration }

#[derive(Debug, Default)]
pub struct ReconcileResult { pub created: Vec<SessionId>, pub reconnected: Vec<SessionId>, pub disconnected: Vec<SessionId>, pub removed: Vec<SessionId> }

impl SessionManager {
    pub fn new(grace: Duration) -> Self { Self { sessions: RwLock::new(HashMap::new()), grace } }

    pub async fn list(&self) -> Vec<Session> {
        let mut sessions = self.sessions.read().await.values().cloned().collect::<Vec<_>>();
        sessions.sort_by_key(|s| (s.project.display_name.clone(), s.endpoint.port)); sessions
    }

    pub async fn get(&self, id: SessionId) -> Option<Session> { self.sessions.read().await.get(&id).cloned() }

    pub async fn set_preview_visible(&self, id: SessionId, visible: bool) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(s) = sessions.get_mut(&id) else { return false; };
        s.preview_visible = visible;
        s.status = if visible || s.status == SessionStatus::Active { SessionStatus::Active } else { SessionStatus::Hidden };
        true
    }

    pub async fn reconcile(&self, discovered: Vec<DiscoveredServer>, now: DateTime<Utc>) -> ReconcileResult {
        let mut sessions = self.sessions.write().await;
        let mut result = ReconcileResult::default();
        let mut seen = HashSet::new();
        for server in discovered {
            let identity = project_identity(&server.candidate);
            let existing_id = sessions.values().find(|s| s.project.key == identity.key && s.classification.kind == server.classification.kind).map(|s| s.id)
                .or_else(|| sessions.values().find(|s| same_endpoint(&s.endpoint, &server.candidate.endpoint)).map(|s| s.id));
            match existing_id {
                Some(id) => {
                    let session = sessions.get_mut(&id).expect("id from map");
                    seen.insert(id);
                    if matches!(session.status, SessionStatus::Disconnected | SessionStatus::Hidden) && session.disconnected_at.is_some() { result.reconnected.push(id); }
                    session.endpoint = server.candidate.endpoint;
                    session.classification = server.classification;
                    session.project = identity;
                    session.status = if session.preview_visible { SessionStatus::Active } else { SessionStatus::Hidden };
                    session.last_seen = now;
                    session.disconnected_at = None;
                }
                None => {
                    let id = Uuid::new_v4(); seen.insert(id); result.created.push(id);
                    sessions.insert(id, Session { id, endpoint:server.candidate.endpoint, classification:server.classification, project:identity, status:SessionStatus::Active, first_seen:now, last_seen:now, disconnected_at:None, preview_visible:false });
                }
            }
        }
        let grace = chrono::Duration::from_std(self.grace).unwrap_or_else(|_| chrono::Duration::seconds(3));
        let ids = sessions.keys().copied().collect::<Vec<_>>();
        for id in ids {
            if seen.contains(&id) { continue; }
            let session = sessions.get_mut(&id).expect("known id");
            if session.disconnected_at.is_none() {
                session.disconnected_at = Some(now); session.status = SessionStatus::Disconnected; result.disconnected.push(id);
            } else if now - session.disconnected_at.unwrap() >= grace {
                sessions.remove(&id); result.removed.push(id);
            }
        }
        result
    }
}

fn same_endpoint(a: &Endpoint, b: &Endpoint) -> bool { a.host == b.host && a.port == b.port }

#[cfg(test)]
mod tests {
    use super::*;
    use localview_protocol::{Classification, ListenerCandidate, ServerKind};
    fn discovered(port:u16) -> DiscoveredServer { DiscoveredServer { candidate:ListenerCandidate { endpoint:Endpoint{host:"127.0.0.1".into(),port,scheme:"http".into()},pid:Some(9),process_name:None,command:Some("vite".into()),cwd:Some("/tmp/app".into()) }, classification:Classification { kind:ServerKind::FrontendDevServer, confidence:1.0, framework:Some("Vite".into()), title:None, hmr_detected:true, evidence:Default::default() } } }
    #[tokio::test]
    async fn reconnects_same_project_when_port_changes() {
        let manager = SessionManager::new(Duration::from_secs(2)); let t = Utc::now();
        let first = manager.reconcile(vec![discovered(5173)], t).await; let id = first.created[0];
        let moved = manager.reconcile(vec![discovered(5174)], t + chrono::Duration::milliseconds(500)).await;
        assert!(moved.created.is_empty()); assert_eq!(manager.get(id).await.unwrap().endpoint.port, 5174);
    }
    #[tokio::test]
    async fn removes_after_grace() {
        let manager = SessionManager::new(Duration::from_secs(1)); let t=Utc::now(); manager.reconcile(vec![discovered(5173)], t).await;
        assert_eq!(manager.reconcile(vec![], t + chrono::Duration::milliseconds(10)).await.disconnected.len(),1);
        assert_eq!(manager.reconcile(vec![], t + chrono::Duration::seconds(2)).await.removed.len(),1);
    }
}
