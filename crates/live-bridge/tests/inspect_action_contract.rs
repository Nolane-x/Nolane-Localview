use localview_live_bridge::BridgeActionKind;

#[test]
fn inspect_action_has_stable_wire_shape() {
    let encoded = serde_json::to_value(BridgeActionKind::Inspect).expect("inspect action serializes");
    assert_eq!(encoded, serde_json::json!({"type": "inspect"}));
}
