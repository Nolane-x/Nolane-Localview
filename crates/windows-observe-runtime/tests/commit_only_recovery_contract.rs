use localview_live_bridge::ConsequentialJournal;
use localview_windows_observe_runtime::{
    WindowsUiaVerifiedExecutionError, recover_consequential_uia_commit_only,
};
use uuid::Uuid;

#[tokio::test]
async fn commit_only_recovery_requires_only_durable_journal_authority() {
    let path = std::env::temp_dir().join(format!(
        "localview-v43-commit-only-recovery-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let action_id = Uuid::new_v4();

    let error = recover_consequential_uia_commit_only(&journal, action_id)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        WindowsUiaVerifiedExecutionError::UnexpectedRecoveryState { state: None }
    ));
    let _ = std::fs::remove_file(path);
}
