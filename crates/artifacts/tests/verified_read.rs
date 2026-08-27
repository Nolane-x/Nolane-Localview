use std::{path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

use localview_artifacts::ArtifactStore;

fn test_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("lv-art-read-{name}-{}-{nonce}", std::process::id()))
}

#[tokio::test]
async fn read_by_content_id_returns_exact_bytes_and_unknown_ids_are_absent() {
    let dir = test_dir("exact");
    let mut store = ArtifactStore::open(&dir, 1024).await.expect("store");
    let meta = store.put("visual/png", b"candidate-png").await.expect("put");

    assert_eq!(
        store.read(&meta.id).await.expect("verified read"),
        Some(b"candidate-png".to_vec())
    );
    assert_eq!(store.read("lv-0000000000000000").await.expect("missing"), None);

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn successful_reads_refresh_lru_without_exposing_or_changing_the_id() {
    let dir = test_dir("lru");
    let mut store = ArtifactStore::open(&dir, 8).await.expect("store");
    let first = store.put("visual/png", b"1111").await.expect("first");
    let second = store.put("visual/png", b"2222").await.expect("second");

    assert_eq!(store.read(&first.id).await.expect("touch first"), Some(b"1111".to_vec()));
    let third = store.put("visual/png", b"3333").await.expect("third");

    assert_eq!(store.read(&first.id).await.expect("first survives"), Some(b"1111".to_vec()));
    assert_eq!(store.read(&second.id).await.expect("second evicted"), None);
    assert_eq!(store.read(&third.id).await.expect("third survives"), Some(b"3333".to_vec()));

    let _ = tokio::fs::remove_dir_all(dir).await;
}

#[tokio::test]
async fn content_id_mismatch_fails_closed_instead_of_returning_tampered_bytes() {
    let dir = test_dir("tamper");
    let mut store = ArtifactStore::open(&dir, 1024).await.expect("store");
    let meta = store.put("visual/png", b"trusted").await.expect("put");
    tokio::fs::write(&meta.path, b"tampered")
        .await
        .expect("tamper fixture");

    assert!(store.read(&meta.id).await.is_err());

    let _ = tokio::fs::remove_dir_all(dir).await;
}
