use std::{path::Path, time::Duration};

use chrono::{TimeZone, Utc};
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::{
    SESSION_IDENTITY_REGISTRY_FILE, SessionIdentityHealth, SessionIdentityResolver, SessionManager,
};
use uuid::Uuid;

fn discovered(port: u16, command: &str) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: "http".into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("localview-test-server".into()),
            command: Some(command.into()),
            cwd: Some("/work/ambiguous-reentry".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("test-framework".into()),
            title: None,
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

fn time(second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 8, 13, 0, second)
        .single()
        .expect("valid test time")
}

fn cleanup(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

#[tokio::test]
async fn ambiguous_current_lineage_cannot_hijack_owner_for_new_endpoint() {
    let dir = std::env::temp_dir().join(format!(
        "localview-ambiguous-lineage-reentry-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp registry directory");
    let path = dir.join(SESSION_IDENTITY_REGISTRY_FILE);

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;
    let observer = resolver.clone();
    let manager = SessionManager::with_identity_resolver(Duration::from_secs(1), resolver);

    let initial = manager
        .reconcile(
            vec![discovered(5173, "vite-a"), discovered(5174, "vite-b")],
            time(0),
        )
        .await;
    assert_eq!(initial.created.len(), 2);
    assert_eq!(observer.record_count(), 0);

    let reentry = manager
        .reconcile(vec![discovered(5175, "vite-c")], time(1))
        .await;

    assert_eq!(
        reentry.created.len(),
        1,
        "a non-exact endpoint must not claim either existing owner while current lineage is ambiguous"
    );
    assert!(!initial.created.contains(&reentry.created[0]));
    assert_eq!(manager.list().await.len(), 3);
    assert_eq!(observer.health(), SessionIdentityHealth::Healthy);
    assert_eq!(
        observer.record_count(),
        0,
        "current-lineage ambiguity must remain volatile and must not publish a durable mapping"
    );

    cleanup(&path);
}
