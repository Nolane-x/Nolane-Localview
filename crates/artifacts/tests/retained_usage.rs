use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use localview_artifacts::ArtifactStore;

fn test_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "lv-art-retained-{name}-{}-{nonce}",
        std::process::id()
    ))
}

async fn disk_bytes(dir: &Path) -> u64 {
    let mut total = 0_u64;
    let mut entries = tokio::fs::read_dir(dir).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        if entry.file_type().await.unwrap().is_file() {
            total += entry.metadata().await.unwrap().len();
        }
    }
    total
}

#[tokio::test]
async fn retained_projection_matches_dedupe_and_lru_result() {
    let dir = test_dir("projection");
    let mut store = ArtifactStore::open(&dir, 5).await.unwrap();

    assert_eq!(store.used_bytes(), 0);
    assert_eq!(store.projected_used_bytes_after_put(b"1234").unwrap(), 4);

    let first = store.put("visual/png", b"1234").await.unwrap();
    assert_eq!(store.used_bytes(), 4);
    assert_eq!(store.projected_used_bytes_after_put(b"1234").unwrap(), 4);

    let duplicate = store.put("visual/png", b"1234").await.unwrap();
    assert_eq!(first.id, duplicate.id);
    assert_eq!(store.used_bytes(), 4);

    assert_eq!(store.projected_used_bytes_after_put(b"5678").unwrap(), 4);
    store.put("visual/png", b"5678").await.unwrap();
    assert_eq!(store.used_bytes(), 4);
    assert_eq!(disk_bytes(&dir).await, 4);

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn oversized_single_artifact_is_rejected_before_file_creation() {
    let dir = test_dir("oversized");
    let mut store = ArtifactStore::open(&dir, 5).await.unwrap();

    assert!(store
        .projected_used_bytes_after_put(b"123456")
        .is_err());
    assert!(store.put("visual/png", b"123456").await.is_err());
    assert_eq!(store.used_bytes(), 0);
    assert_eq!(disk_bytes(&dir).await, 0);

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn failed_gc_deletion_is_not_laundered_as_reclaimed_usage() {
    let dir = test_dir("gc-delete-failure");
    let mut store = ArtifactStore::open(&dir, 5).await.unwrap();
    let first = store.put("visual/png", b"1234").await.unwrap();

    tokio::fs::remove_file(&first.path).await.unwrap();
    tokio::fs::create_dir(&first.path).await.unwrap();

    let result = store.put("visual/png", b"5678").await;
    assert!(
        result.is_err(),
        "a failed eviction must fail the store transaction instead of hiding retained bytes"
    );
    assert_eq!(
        store.used_bytes(),
        8,
        "the failed-to-delete entry and the newly written entry must both remain accounted"
    );
    assert!(Path::new(&first.path).is_dir());
    assert_eq!(disk_bytes(&dir).await, 4);

    let _ = tokio::fs::remove_dir_all(dir).await;
}
