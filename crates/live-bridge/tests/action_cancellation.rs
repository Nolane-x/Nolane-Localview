use chrono::Utc;
use localview_live_bridge::{
    ActionCancellationState, BridgeActionKind, BridgeActionResult, LiveBridge,
};
use serde_json::Value;
use uuid::Uuid;

#[tokio::test]
async fn pending_public_action_cancel_is_terminal_and_filters_delivery() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let action = bridge
        .enqueue_action(session, Some("@save".into()), BridgeActionKind::Click)
        .await;

    let cancelled = bridge
        .request_action_cancellation(session, action.id)
        .await
        .expect("pending action remains cancellable");
    assert_eq!(cancelled.state, ActionCancellationState::Cancelled);
    assert!(cancelled.acknowledged);
    assert!(bridge.take_actions(session, 8).await.is_empty());
    assert!(bridge.recent_results(session, 8).await.is_empty());

    let duplicate = bridge
        .request_action_cancellation(session, action.id)
        .await
        .expect("terminal tombstone makes cancellation idempotent");
    assert_eq!(duplicate.state, ActionCancellationState::Cancelled);
    assert!(duplicate.acknowledged);
}

#[tokio::test]
async fn inflight_cancel_fences_result_claim_before_ack() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let action = bridge
        .enqueue_action(session, Some("@save".into()), BridgeActionKind::Click)
        .await;
    assert_eq!(bridge.take_actions(session, 8).await.len(), 1);

    let outcome = bridge
        .request_action_cancellation(session, action.id)
        .await
        .expect("inflight action remains cancellable");
    assert_eq!(outcome.state, ActionCancellationState::CancellationRequested);
    assert!(!outcome.acknowledged);
    assert!(bridge.claim_action(session, action.id).await.is_none());

    let signal = bridge
        .action_cancellation(session, action.id)
        .await
        .expect("exact signal");
    assert_eq!(signal.action_id, action.id);

    assert!(bridge
        .acknowledge_action_cancellation(session, action.id)
        .await);
    assert!(bridge.action_cancellation(session, action.id).await.is_none());
    assert!(bridge.recent_results(session, 8).await.is_empty());
}

#[tokio::test]
async fn cancellation_ack_discards_origin_instead_of_polluting_claimed_storage() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let secret = "must-remain-redacted";

    let protected = bridge
        .enqueue_action(
            session,
            Some("@secret".into()),
            BridgeActionKind::TypeText {
                text: secret.into(),
                clear_first: true,
            },
        )
        .await;
    assert_eq!(bridge.take_actions(session, 8).await.len(), 1);
    assert_eq!(
        bridge
            .claim_action(session, protected.id)
            .await
            .expect("protected result origin")
            .id,
        protected.id
    );

    for _ in 0..8 {
        let cancelled = bridge
            .enqueue_action(session, None, BridgeActionKind::Click)
            .await;
        assert_eq!(bridge.take_actions(session, 8).await.len(), 1);
        let outcome = bridge
            .request_action_cancellation(session, cancelled.id)
            .await
            .expect("inflight cancellation");
        assert_eq!(outcome.state, ActionCancellationState::CancellationRequested);
        assert!(bridge
            .acknowledge_action_cancellation(session, cancelled.id)
            .await);
    }

    bridge
        .complete_action(
            session,
            BridgeActionResult {
                action_id: protected.id,
                ok: true,
                error: Some(format!("echo:{secret}")),
                payload: serde_json::json!({"value": secret}),
                completed_at: Utc::now(),
            },
        )
        .await;

    let result = bridge
        .recent_results(session, 8)
        .await
        .into_iter()
        .find(|result| result.action_id == protected.id)
        .expect("protected completion result");
    assert_eq!(result.payload, Value::Null);
    assert_eq!(result.error.as_deref(), Some("echo:[REDACTED]"));
}

#[tokio::test]
async fn result_claim_wins_linearization_and_later_cancel_is_too_late() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let action = bridge
        .enqueue_action(session, None, BridgeActionKind::Snapshot)
        .await;
    assert_eq!(bridge.take_actions(session, 8).await.len(), 1);

    let claimed = bridge
        .claim_action(session, action.id)
        .await
        .expect("result claim wins before cancellation");
    assert_eq!(claimed.id, action.id);
    assert!(bridge
        .request_action_cancellation(session, action.id)
        .await
        .is_none());

    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: action.id,
                ok: true,
                error: None,
                payload: Value::Null,
                completed_at: Utc::now(),
            },
        )
        .await;
    assert_eq!(bridge.recent_results(session, 8).await.len(), 1);
}

#[tokio::test]
async fn cancellation_is_exact_session_owned() {
    let bridge = LiveBridge::new(64, 8);
    let owner = Uuid::new_v4();
    let other = Uuid::new_v4();
    let action = bridge
        .enqueue_action(owner, None, BridgeActionKind::Focus)
        .await;

    assert!(bridge
        .request_action_cancellation(other, action.id)
        .await
        .is_none());
    assert_eq!(bridge.take_actions(owner, 8).await.len(), 1);
}

#[tokio::test]
async fn queue_eviction_removes_stale_pending_cancellation_ownership() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let mut ids = Vec::new();
    for _ in 0..9 {
        ids.push(
            bridge
                .enqueue_action(session, None, BridgeActionKind::Click)
                .await
                .id,
        );
    }

    assert!(bridge
        .request_action_cancellation(session, ids[0])
        .await
        .is_none());
    assert!(bridge
        .request_action_cancellation(session, ids[8])
        .await
        .is_some());
}

#[tokio::test]
async fn cancellation_listing_is_bounded_but_exact_lookup_is_not_truncated() {
    let bridge = LiveBridge::new(128, 64);
    let session = Uuid::new_v4();
    let mut ids = Vec::new();
    for _ in 0..40 {
        ids.push(
            bridge
                .enqueue_action(session, None, BridgeActionKind::Click)
                .await
                .id,
        );
    }
    assert_eq!(bridge.take_actions(session, 64).await.len(), 40);
    for id in &ids {
        let outcome = bridge
            .request_action_cancellation(session, *id)
            .await
            .expect("inflight action");
        assert_eq!(outcome.state, ActionCancellationState::CancellationRequested);
    }

    assert_eq!(bridge.action_cancellations(session, 32).await.len(), 32);
    assert_eq!(
        bridge
            .action_cancellation(session, ids[39])
            .await
            .expect("exact lookup beyond listing")
            .action_id,
        ids[39]
    );
}

#[tokio::test]
async fn terminal_action_cancellation_tombstones_are_bounded() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let mut first = None;
    let mut last = None;
    for index in 0..300 {
        let action = bridge
            .enqueue_action(session, None, BridgeActionKind::Click)
            .await;
        if index == 0 {
            first = Some(action.id);
        }
        last = Some(action.id);
        assert!(bridge
            .request_action_cancellation(session, action.id)
            .await
            .is_some());
    }

    assert!(bridge
        .request_action_cancellation(session, first.expect("first"))
        .await
        .is_none());
    let latest = bridge
        .request_action_cancellation(session, last.expect("last"))
        .await
        .expect("latest tombstone retained");
    assert_eq!(latest.state, ActionCancellationState::Cancelled);
}

#[tokio::test]
async fn public_cancellation_cannot_address_internal_capture_actions() {
    let bridge = LiveBridge::new(64, 8);
    let session = Uuid::new_v4();
    let freeze = bridge
        .enqueue_capture_freeze(session, vec![".secret".into()])
        .await;

    assert!(bridge
        .request_action_cancellation(session, freeze.id)
        .await
        .is_none());
    let internal = bridge.take_internal_capture_actions(session, 8).await;
    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].id, freeze.id);
    assert!(bridge.claim_action(session, freeze.id).await.is_some());
}
