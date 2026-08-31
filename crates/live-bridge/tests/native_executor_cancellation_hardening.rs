use localview_live_bridge::{
    LiveBridge, NativeExecutorAction, NativeExecutorCancellationState,
};
use localview_protocol::{SessionId, ViewportMeta};

fn action() -> NativeExecutorAction {
    NativeExecutorAction::VisualDiffCapture {
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.0,
        },
        revision: Some("cancel-hardening".into()),
    }
}

#[tokio::test]
async fn evicted_pending_origin_is_not_a_cancellation_target() {
    let bridge = LiveBridge::new(16, 8);
    let session_id = SessionId::new_v4();
    let oldest = bridge.enqueue_native_executor(session_id, action()).await;
    let mut retained = Vec::new();
    for _ in 0..8 {
        retained.push(bridge.enqueue_native_executor(session_id, action()).await);
    }

    assert!(
        bridge
            .request_native_executor_cancellation(session_id, oldest.id)
            .await
            .is_none(),
        "the bounded native queue already evicted this origin, so cancellation authority must not retain a stale owner"
    );

    let middle = retained[3].id;
    let middle_cancel = bridge
        .request_native_executor_cancellation(session_id, middle)
        .await
        .expect("middle request remains queued");
    assert_eq!(middle_cancel.state, NativeExecutorCancellationState::Cancelled);
    assert!(middle_cancel.acknowledged);

    let newest = retained.last().expect("newest retained request").id;
    let newest_cancel = bridge
        .request_native_executor_cancellation(session_id, newest)
        .await
        .expect("newest request remains queued");
    assert_eq!(newest_cancel.state, NativeExecutorCancellationState::Cancelled);
    assert!(newest_cancel.acknowledged);
}

#[tokio::test]
async fn terminal_cancellation_tombstones_are_bounded() {
    let bridge = LiveBridge::new(16, 8);
    let session_id = SessionId::new_v4();
    let mut first_id = None;
    let mut last_id = None;

    for index in 0..320 {
        let request = bridge.enqueue_native_executor(session_id, action()).await;
        if index == 0 {
            first_id = Some(request.id);
        }
        last_id = Some(request.id);
        let outcome = bridge
            .request_native_executor_cancellation(session_id, request.id)
            .await
            .expect("fresh queued request can be cancelled");
        assert_eq!(outcome.state, NativeExecutorCancellationState::Cancelled);
        assert!(outcome.acknowledged);
    }

    assert!(
        bridge
            .request_native_executor_cancellation(session_id, first_id.expect("first id"))
            .await
            .is_none(),
        "old terminal cancellation tombstones must age out of bounded retention"
    );

    let newest = bridge
        .request_native_executor_cancellation(session_id, last_id.expect("last id"))
        .await
        .expect("newest tombstone remains retained for idempotency");
    assert_eq!(newest.state, NativeExecutorCancellationState::Cancelled);
    assert!(newest.acknowledged);
}
