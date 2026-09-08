use std::{path::{Path, PathBuf}, time::Duration};

use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind, SessionStatus,
};
use localview_sessions::{
    SESSION_IDENTITY_REGISTRY_FILE, SessionIdentityHealth, SessionIdentityResolver, SessionManager,
};
use uuid::Uuid;

fn temp_registry() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "localview-durable-session-continuity-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp registry directory");
    let path = dir.join(SESSION_IDENTITY_REGISTRY_FILE);
    (dir, path)
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn discovered(
    cwd: Option<&str>,
    port: u16,
    scheme: &str,
    command: Option<&str>,
    kind: ServerKind,
) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: scheme.into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("localview-test-server".into()),
            command: command.map(str::to_owned),
            cwd: cwd.map(str::to_owned),
        },
        classification: Classification {
            kind,
            confidence: 1.0,
            framework: Some("test-framework".into()),
            title: None,
            hmr_detected: matches!(kind, ServerKind::FrontendDevServer),
            evidence: Default::default(),
        },
    }
}

fn time(second: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 8, 12, 0, second)
        .single()
        .expect("valid test time")
}

#[tokio::test]
async fn same_project_reuses_uuid_across_fresh_manager_lifetimes() {
    let (dir, path) = temp_registry();
    let first_resolver = SessionIdentityResolver::open_file(path.clone()).await;
    let first_manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        first_resolver,
    );
    let first = first_manager
        .reconcile(
            vec![discovered(
                Some("/work/app"),
                5173,
                "http",
                Some("vite"),
                ServerKind::FrontendDevServer,
            )],
            time(0),
        )
        .await;
    let first_id = first.created[0];
    drop(first_manager);

    let reopened = SessionIdentityResolver::open_file(path.clone()).await;
    let second_manager = SessionManager::with_identity_resolver(Duration::from_secs(1), reopened);
    let second = second_manager
        .reconcile(
            vec![discovered(
                Some("/work/app"),
                8443,
                "https",
                Some("vite --host 0.0.0.0"),
                ServerKind::FrontendDevServer,
            )],
            time(1),
        )
        .await;

    assert_eq!(second.created, vec![first_id]);
    let session = second_manager.get(first_id).await.expect("reused session id");
    assert_eq!(session.endpoint.port, 8443);
    assert_eq!(session.endpoint.scheme, "https");
    cleanup(&dir);
}

#[tokio::test]
async fn current_process_command_and_port_change_keeps_uuid() {
    let (dir, path) = temp_registry();
    let resolver = SessionIdentityResolver::open_file(path).await;
    let manager = SessionManager::with_identity_resolver(Duration::from_secs(1), resolver);
    let first = manager
        .reconcile(
            vec![discovered(
                Some("/work/app"),
                5173,
                "http",
                Some("vite"),
                ServerKind::FrontendDevServer,
            )],
            time(0),
        )
        .await;
    let id = first.created[0];

    let moved = manager
        .reconcile(
            vec![discovered(
                Some("/work/app"),
                5174,
                "http",
                Some("vite --host"),
                ServerKind::FrontendDevServer,
            )],
            time(1),
        )
        .await;

    assert!(moved.created.is_empty());
    assert_eq!(manager.list().await.len(), 1);
    assert_eq!(manager.get(id).await.unwrap().endpoint.port, 5174);
    cleanup(&dir);
}

#[tokio::test]
async fn projectless_port_change_gets_new_uuid_after_restart() {
    let (dir, path) = temp_registry();
    let first_manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path.clone()).await,
    );
    let first = first_manager
        .reconcile(
            vec![discovered(
                None,
                7000,
                "http",
                None,
                ServerKind::UnknownHttp,
            )],
            time(0),
        )
        .await;
    let first_id = first.created[0];
    drop(first_manager);

    let second_manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path).await,
    );
    let second = second_manager
        .reconcile(
            vec![discovered(
                None,
                7001,
                "http",
                None,
                ServerKind::UnknownHttp,
            )],
            time(1),
        )
        .await;
    let second_id = second.created[0];

    assert_ne!(second_id, first_id);
    cleanup(&dir);
}

#[tokio::test]
async fn same_project_root_with_different_server_kind_gets_distinct_ids() {
    let (dir, path) = temp_registry();
    let manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path).await,
    );

    let result = manager
        .reconcile(
            vec![
                discovered(
                    Some("/work/monorepo"),
                    5173,
                    "http",
                    Some("vite"),
                    ServerKind::FrontendDevServer,
                ),
                discovered(
                    Some("/work/monorepo"),
                    6006,
                    "http",
                    Some("storybook"),
                    ServerKind::Storybook,
                ),
            ],
            time(0),
        )
        .await;

    assert_eq!(result.created.len(), 2);
    assert_ne!(result.created[0], result.created[1]);
    cleanup(&dir);
}

#[tokio::test]
async fn ambiguous_same_lineage_batch_never_aliases_or_claims_durable_mapping() {
    let (dir, path) = temp_registry();
    let resolver = SessionIdentityResolver::open_file(path).await;
    let observer = resolver.clone();
    let manager = SessionManager::with_identity_resolver(Duration::from_secs(1), resolver);

    let result = manager
        .reconcile(
            vec![
                discovered(
                    Some("/work/ambiguous"),
                    5173,
                    "http",
                    Some("vite-a"),
                    ServerKind::FrontendDevServer,
                ),
                discovered(
                    Some("/work/ambiguous"),
                    5174,
                    "http",
                    Some("vite-b"),
                    ServerKind::FrontendDevServer,
                ),
            ],
            time(0),
        )
        .await;

    assert_eq!(result.created.len(), 2);
    assert_ne!(result.created[0], result.created[1]);
    assert_eq!(manager.list().await.len(), 2);
    assert_eq!(observer.health(), SessionIdentityHealth::Healthy);
    assert_eq!(observer.record_count(), 0, "ambiguous lineage must not be persisted");
    cleanup(&dir);
}

#[tokio::test]
async fn removed_session_keeps_durable_mapping_for_later_restart() {
    let (dir, path) = temp_registry();
    let manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path.clone()).await,
    );
    let created = manager
        .reconcile(
            vec![discovered(
                Some("/work/returning"),
                5173,
                "http",
                Some("vite"),
                ServerKind::FrontendDevServer,
            )],
            time(0),
        )
        .await;
    let id = created.created[0];
    manager.reconcile(vec![], time(1)).await;
    let removed = manager
        .reconcile(vec![], time(1) + ChronoDuration::seconds(2))
        .await;
    assert_eq!(removed.removed, vec![id]);
    drop(manager);

    let restarted = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path).await,
    );
    let returned = restarted
        .reconcile(
            vec![discovered(
                Some("/work/returning"),
                9000,
                "https",
                Some("new-command"),
                ServerKind::FrontendDevServer,
            )],
            time(4),
        )
        .await;

    assert_eq!(returned.created, vec![id]);
    cleanup(&dir);
}

#[tokio::test]
async fn reused_uuid_starts_with_fresh_runtime_state_not_persisted_session_state() {
    let (dir, path) = temp_registry();
    let first_manager = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path.clone()).await,
    );
    let first = first_manager
        .reconcile(
            vec![discovered(
                Some("/work/fresh-runtime"),
                5173,
                "http",
                Some("vite"),
                ServerKind::FrontendDevServer,
            )],
            time(0),
        )
        .await;
    let id = first.created[0];
    assert!(first_manager.set_preview_visible(id, true).await);
    assert!(first_manager.get(id).await.unwrap().preview_visible);
    drop(first_manager);

    let restarted_at = time(5);
    let restarted = SessionManager::with_identity_resolver(
        Duration::from_secs(1),
        SessionIdentityResolver::open_file(path).await,
    );
    let result = restarted
        .reconcile(
            vec![discovered(
                Some("/work/fresh-runtime"),
                5174,
                "http",
                Some("vite"),
                ServerKind::FrontendDevServer,
            )],
            restarted_at,
        )
        .await;
    assert_eq!(result.created, vec![id]);

    let session = restarted.get(id).await.expect("recreated runtime session");
    assert_eq!(session.status, SessionStatus::Active);
    assert!(!session.preview_visible);
    assert_eq!(session.first_seen, restarted_at);
    assert_eq!(session.last_seen, restarted_at);
    assert_eq!(session.disconnected_at, None);
    cleanup(&dir);
}
