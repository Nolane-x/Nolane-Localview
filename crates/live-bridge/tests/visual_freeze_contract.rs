use chrono::Utc;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use uuid::Uuid;

#[test]
fn internal_visual_actions_have_stable_wire_shapes() {
    let freeze = serde_json::to_value(BridgeActionKind::FreezeVisuals).unwrap();
    assert_eq!(freeze, serde_json::json!({"type": "freeze_visuals"}));

    let token = Uuid::from_u128(1);
    let restore = serde_json::to_value(BridgeActionKind::RestoreVisuals { token }).unwrap();
    assert_eq!(
        restore,
        serde_json::json!({
            "type": "restore_visuals",
            "token": "00000000-0000-0000-0000-000000000001"
        })
    );
}

#[test]
fn only_visual_state_actions_are_internal_capture_actions() {
    assert!(BridgeActionKind::FreezeVisuals.is_internal_capture_action());
    assert!(BridgeActionKind::RestoreVisuals {
        token: Uuid::from_u128(1),
    }
    .is_internal_capture_action());

    assert!(!BridgeActionKind::Click.is_internal_capture_action());
    assert!(!BridgeActionKind::Snapshot.is_internal_capture_action());
}

#[tokio::test]
async fn public_and_internal_drains_never_steal_each_others_actions() {
    let bridge = LiveBridge::new(64, 16);
    let session_id = Uuid::from_u128(7);
    let click = bridge
        .enqueue_action(session_id, Some("@button".into()), BridgeActionKind::Click)
        .await;
    let freeze = bridge
        .enqueue_action(session_id, None, BridgeActionKind::FreezeVisuals)
        .await;
    let snapshot = bridge
        .enqueue_action(session_id, None, BridgeActionKind::Snapshot)
        .await;
    let restore = bridge
        .enqueue_action(
            session_id,
            None,
            BridgeActionKind::RestoreVisuals { token: freeze.id },
        )
        .await;

    let public = bridge.take_public_actions(session_id, 16).await;
    assert_eq!(
        public.iter().map(|action| action.id).collect::<Vec<_>>(),
        vec![click.id, snapshot.id]
    );
    assert!(public
        .iter()
        .all(|action| !action.action.is_internal_capture_action()));

    let internal = bridge.take_internal_capture_actions(session_id, 16).await;
    assert_eq!(
        internal.iter().map(|action| action.id).collect::<Vec<_>>(),
        vec![freeze.id, restore.id]
    );
    assert!(internal
        .iter()
        .all(|action| action.action.is_internal_capture_action()));
}

#[tokio::test]
async fn public_queue_pressure_cannot_evict_queued_internal_capture_actions() {
    let bridge = LiveBridge::new(64, 8);
    let session_id = Uuid::from_u128(8);
    let freeze = bridge
        .enqueue_action(session_id, None, BridgeActionKind::FreezeVisuals)
        .await;

    for _ in 0..32 {
        bridge
            .enqueue_action(session_id, None, BridgeActionKind::Click)
            .await;
    }

    let internal = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].id, freeze.id);
}

#[tokio::test]
async fn public_inflight_pressure_cannot_evict_internal_capture_origin() {
    let bridge = LiveBridge::new(64, 8);
    let session_id = Uuid::from_u128(9);
    let freeze = bridge
        .enqueue_action(session_id, None, BridgeActionKind::FreezeVisuals)
        .await;
    let internal = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(internal.len(), 1);

    for _ in 0..24 {
        bridge
            .enqueue_action(session_id, None, BridgeActionKind::Click)
            .await;
        let _ = bridge.take_public_actions(session_id, 8).await;
    }

    assert_eq!(
        bridge
            .claim_action(session_id, freeze.id)
            .await
            .map(|action| action.id),
        Some(freeze.id)
    );
}

#[tokio::test]
async fn internal_queue_pressure_cannot_evict_queued_public_actions() {
    let bridge = LiveBridge::new(64, 8);
    let session_id = Uuid::from_u128(10);
    let click = bridge
        .enqueue_action(session_id, Some("@button".into()), BridgeActionKind::Click)
        .await;

    for _ in 0..24 {
        bridge
            .enqueue_action(session_id, None, BridgeActionKind::FreezeVisuals)
            .await;
    }

    let public = bridge.take_public_actions(session_id, 8).await;
    assert_eq!(public.len(), 1);
    assert_eq!(public[0].id, click.id);
}

#[tokio::test]
async fn internal_capture_results_never_appear_in_public_result_history() {
    let bridge = LiveBridge::new(64, 8);
    let session_id = Uuid::from_u128(11);
    let freeze = bridge
        .enqueue_action(session_id, None, BridgeActionKind::FreezeVisuals)
        .await;
    let mut internal = bridge.take_internal_capture_actions(session_id, 8).await;
    let action = internal.pop().expect("freeze action must drain privately");
    let claimed = bridge
        .claim_action(session_id, action.id)
        .await
        .expect("freeze action origin must be claimable");
    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: claimed.id,
                ok: true,
                error: None,
                payload: serde_json::json!({
                    "paused_animations": 3,
                    "web_animations_supported": true
                }),
                completed_at: Utc::now(),
            },
        )
        .await;

    assert!(bridge.recent_results(session_id, 8).await.is_empty());
    let private = bridge
        .recent_internal_capture_results(session_id, 8)
        .await;
    assert_eq!(private.len(), 1);
    assert_eq!(private[0].action_id, freeze.id);
}
