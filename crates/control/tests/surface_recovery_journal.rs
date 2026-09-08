#![forbid(unsafe_code)]

use std::path::PathBuf;

use localview_control::{SurfaceRecoveryJournal, SurfaceRecoveryKey};
use uuid::Uuid;

fn temp_journal_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-surface-recovery-{label}-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn recovery_key(owner_instance_id: Uuid) -> SurfaceRecoveryKey {
    SurfaceRecoveryKey::new(
        Uuid::new_v4(),
        "preview_window",
        "preview-recovery-contract",
        7,
        owner_instance_id,
    )
    .expect("valid exact recovery key")
}

#[tokio::test]
async fn activated_and_released_events_replay_exact_recovery_debt() {
    let path = temp_journal_path("replay-release");
    let key = recovery_key(Uuid::new_v4());

    let journal = SurfaceRecoveryJournal::open(&path)
        .await
        .expect("open recovery journal");
    journal
        .record_activated(key.clone())
        .await
        .expect("durably record activation");
    assert!(journal.outstanding_exact(&key));
    drop(journal);

    let reopened = SurfaceRecoveryJournal::open(&path)
        .await
        .expect("replay activation debt");
    assert!(reopened.outstanding_exact(&key));
    reopened
        .record_released(key.clone())
        .await
        .expect("durably discharge release debt");
    drop(reopened);

    let clean = SurfaceRecoveryJournal::open(&path)
        .await
        .expect("replay released journal");
    assert!(!clean.outstanding_exact(&key));
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn reattach_and_release_are_exact_owner_scoped() {
    let path = temp_journal_path("exact-owner");
    let owner_a = Uuid::new_v4();
    let owner_b = Uuid::new_v4();
    let key_a = recovery_key(owner_a);
    let key_b = SurfaceRecoveryKey::new(
        key_a.session_id,
        key_a.surface_kind.clone(),
        key_a.label.clone(),
        key_a.incarnation,
        owner_b,
    )
    .expect("same logical surface under a different owner");

    let journal = SurfaceRecoveryJournal::open(&path)
        .await
        .expect("open recovery journal");
    journal
        .record_activated(key_a.clone())
        .await
        .expect("record owner A activation");
    journal
        .record_released(key_b.clone())
        .await
        .expect("unrelated exact release remains a valid journal event");
    assert!(
        journal.outstanding_exact(&key_a),
        "a different owner instance must not discharge predecessor debt"
    );

    journal
        .record_reattached(key_a.clone())
        .await
        .expect("exact owner reattach discharges boot debt");
    assert!(!journal.outstanding_exact(&key_a));
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn corrupt_recovery_journal_fails_closed() {
    let path = temp_journal_path("corrupt");
    tokio::fs::write(&path, b"{not-valid-json}\n")
        .await
        .expect("write corrupt journal fixture");

    assert!(
        SurfaceRecoveryJournal::open(&path).await.is_err(),
        "corruption must never be laundered into an empty recovery inventory"
    );
    let _ = tokio::fs::remove_file(path).await;
}
