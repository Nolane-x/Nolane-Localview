use std::path::{Path, PathBuf};

use localview_protocol::{Endpoint, ProjectIdentity, ServerKind};
use localview_sessions::{
    MAX_SESSION_IDENTITY_RECORDS, SESSION_IDENTITY_REGISTRY_FILE, SessionIdentityDurability,
    SessionIdentityHealth, SessionIdentityResolver, session_lineage,
};
use serde_json::json;
use uuid::Uuid;

fn temp_registry() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "localview-session-identity-resolver-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp registry directory");
    let path = dir.join(SESSION_IDENTITY_REGISTRY_FILE);
    (dir, path)
}

fn cleanup_path(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

fn project_lineage(path: &str) -> localview_sessions::SessionLineage {
    let project = ProjectIdentity {
        key: "legacy-key-is-not-authority".into(),
        display_name: "app".into(),
        cwd: Some(path.into()),
        git_root: Some(path.into()),
        pid: Some(1234),
        command: Some("vite".into()),
    };
    session_lineage(
        &project,
        &Endpoint {
            host: "127.0.0.1".into(),
            port: 5173,
            scheme: "http".into(),
        },
        ServerKind::FrontendDevServer,
    )
    .expect("canonical lineage")
}

#[tokio::test]
async fn existing_mapping_reuses_exact_uuid_across_resolver_lifetimes() {
    let (dir, path) = temp_registry();
    let lineage = project_lineage("/work/app");

    let first_resolver = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(first_resolver.existing(&lineage).await, None);
    let first = first_resolver.resolve_new(&lineage).await;
    assert_eq!(first.durability, SessionIdentityDurability::Durable);
    assert_ne!(first.session_id, Uuid::nil());
    drop(first_resolver);

    let reopened = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(reopened.health(), SessionIdentityHealth::Healthy);
    assert_eq!(reopened.existing(&lineage).await, Some(first.session_id));
    let second = reopened.resolve_new(&lineage).await;
    assert_eq!(second.durability, SessionIdentityDurability::Durable);
    assert_eq!(second.session_id, first.session_id);

    cleanup_path(&dir);
}

#[tokio::test]
async fn durable_result_is_backed_by_committed_registry_before_return() {
    let (dir, path) = temp_registry();
    let lineage = project_lineage("/work/commit-before-publish");
    let resolver = SessionIdentityResolver::open_file(path.clone()).await;

    let resolved = resolver.resolve_new(&lineage).await;

    assert_eq!(resolved.durability, SessionIdentityDurability::Durable);
    assert!(
        path.is_file(),
        "durable result must already have an on-disk registry"
    );
    let reopened = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(reopened.existing(&lineage).await, Some(resolved.session_id));

    cleanup_path(&dir);
}

#[tokio::test]
async fn commit_failure_returns_volatile_uuid_without_publishing_mapping() {
    let (dir, path) = temp_registry();
    let lineage = project_lineage("/work/commit-failure");
    let resolver = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(resolver.health(), SessionIdentityHealth::Healthy);

    std::fs::remove_dir_all(&dir).expect("remove registry parent after healthy open");
    std::fs::write(&dir, b"parent-is-now-a-file").expect("replace parent directory with file");

    let resolved = resolver.resolve_new(&lineage).await;

    assert_eq!(resolved.durability, SessionIdentityDurability::Volatile);
    assert_ne!(resolved.session_id, Uuid::nil());
    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert_eq!(resolver.existing(&lineage).await, None);
    assert!(
        !path.exists(),
        "failed commit must not publish a registry entry"
    );

    cleanup_path(&dir);
}

#[tokio::test]
async fn full_registry_returns_volatile_without_evicting_existing_records() {
    let (dir, path) = temp_registry();
    let mut records = Vec::with_capacity(MAX_SESSION_IDENTITY_RECORDS);
    let mut first_lineage = None;
    let mut first_id = None;
    for index in 0..MAX_SESSION_IDENTITY_RECORDS {
        let lineage = project_lineage(&format!("/work/full-{index:04}"));
        let session_id = Uuid::new_v4();
        if index == 0 {
            first_lineage = Some(lineage.clone());
            first_id = Some(session_id);
        }
        records.push(json!({
            "lineage": lineage,
            "session_id": session_id,
        }));
    }
    let bytes = serde_json::to_vec(&json!({
        "schema_version": 1,
        "records": records,
    }))
    .expect("serialize full registry");
    std::fs::write(&path, &bytes).expect("write full registry");

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(resolver.health(), SessionIdentityHealth::Healthy);
    assert_eq!(resolver.record_count(), MAX_SESSION_IDENTITY_RECORDS);
    let new_lineage = project_lineage("/work/one-too-many");

    let resolved = resolver.resolve_new(&new_lineage).await;

    assert_eq!(resolved.durability, SessionIdentityDurability::Volatile);
    assert_eq!(resolver.existing(&new_lineage).await, None);
    assert_eq!(
        resolver
            .existing(&first_lineage.expect("first lineage"))
            .await,
        first_id
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);

    cleanup_path(&dir);
}
