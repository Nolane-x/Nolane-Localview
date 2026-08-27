use localview_live_bridge::{LiveBridge, NativeExecutorAction};
use uuid::Uuid;

#[tokio::test]
async fn visual_diff_requests_stay_inside_the_native_executor_queue() {
    let bridge = LiveBridge::new(64, 8);
    let session_id = Uuid::new_v4();

    let request = bridge
        .enqueue_native_executor(
            session_id,
            NativeExecutorAction::VisualDiff {
                baseline_artifact_id: "lv-1111111111111111".into(),
                candidate_artifact_id: "lv-2222222222222222".into(),
                pixel_threshold: 8,
            },
        )
        .await;

    assert!(bridge.take_actions(session_id, 8).await.is_empty());
    let native = bridge.take_native_executor_requests(session_id, 8).await;
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].id, request.id);
    assert_eq!(native[0].session_id, session_id);
    assert!(matches!(
        &native[0].action,
        NativeExecutorAction::VisualDiff {
            baseline_artifact_id,
            candidate_artifact_id,
            pixel_threshold: 8,
        } if baseline_artifact_id == "lv-1111111111111111"
            && candidate_artifact_id == "lv-2222222222222222"
    ));
}
