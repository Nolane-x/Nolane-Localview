use localview_live_bridge::{BridgeActionKind, LiveBridge};
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
