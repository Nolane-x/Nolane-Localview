use std::path::{Path, PathBuf};

use localview_sessions::{
    SESSION_IDENTITY_REGISTRY_FILE, SessionIdentityHealth, SessionIdentityResolver,
};
use uuid::Uuid;

const MAX_REGISTRY_BYTES: usize = 1_048_576;

fn temp_registry() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "localview-session-identity-registry-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp registry directory");
    let path = dir.join(SESSION_IDENTITY_REGISTRY_FILE);
    (dir, path)
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn project_record(path: &str, session_id: Uuid) -> String {
    format!(
        r#"{{"lineage":{{"lineage_version":"localview_session_lineage_v1","value":{{"anchor":{{"anchor_type":"project","normalized_project_path":"{path}"}},"server_kind":"frontend_dev_server"}}}},"session_id":"{session_id}"}}"#
    )
}

#[tokio::test]
async fn missing_registry_opens_healthy() {
    let (dir, path) = temp_registry();
    assert!(!path.exists());

    let resolver = SessionIdentityResolver::open_file(path).await;

    assert_eq!(resolver.health(), SessionIdentityHealth::Healthy);
    assert!(resolver.diagnostic().is_none());
    cleanup(&dir);
}

#[tokio::test]
async fn corrupt_registry_is_preserved_and_enters_volatile_mode() {
    let (dir, path) = temp_registry();
    let original = b"{ definitely-not-json";
    std::fs::write(&path, original).expect("write corrupt registry");

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;

    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert!(resolver.diagnostic().is_some());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    cleanup(&dir);
}

#[tokio::test]
async fn unknown_schema_version_is_preserved_and_degraded() {
    let (dir, path) = temp_registry();
    let original = br#"{"schema_version":999,"records":[]}"#;
    std::fs::write(&path, original).expect("write future registry");

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;

    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    cleanup(&dir);
}

#[tokio::test]
async fn duplicate_lineage_or_uuid_is_rejected_without_rewrite() {
    let (dir, path) = temp_registry();
    let first_id = Uuid::new_v4();
    let second_id = Uuid::new_v4();
    let duplicate_lineage = format!(
        r#"{{"schema_version":1,"records":[{},{}]}}"#,
        project_record("/work/app", first_id),
        project_record("/work/app", second_id),
    );
    std::fs::write(&path, duplicate_lineage.as_bytes()).unwrap();

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), duplicate_lineage);

    let same_uuid_two_lineages = format!(
        r#"{{"schema_version":1,"records":[{},{}]}}"#,
        project_record("/work/app-a", first_id),
        project_record("/work/app-b", first_id),
    );
    std::fs::write(&path, same_uuid_two_lineages.as_bytes()).unwrap();

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;
    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        same_uuid_two_lineages
    );
    cleanup(&dir);
}

#[tokio::test]
async fn oversized_registry_is_rejected_without_rewrite() {
    let (dir, path) = temp_registry();
    let original = vec![b'x'; MAX_REGISTRY_BYTES + 1];
    std::fs::write(&path, &original).unwrap();

    let resolver = SessionIdentityResolver::open_file(path.clone()).await;

    assert_eq!(resolver.health(), SessionIdentityHealth::VolatileDegraded);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    cleanup(&dir);
}
