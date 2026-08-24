use localview_live_bridge::BridgeActionKind;
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
