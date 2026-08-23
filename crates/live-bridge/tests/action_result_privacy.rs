use chrono::Utc;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use uuid::Uuid;

#[tokio::test]
async fn completed_type_text_result_never_retains_typed_value() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let secret = "super-secret-value";

    let queued = bridge
        .enqueue_action(
            session_id,
            Some("@e1".into()),
            BridgeActionKind::TypeText {
                text: secret.into(),
                clear_first: true,
            },
        )
        .await;
    assert_eq!(bridge.take_actions(session_id, 1).await.len(), 1);
    let claimed = bridge
        .claim_action(session_id, queued.id)
        .await
        .expect("queued action must have an inflight origin");

    let completed_at = Utc::now();
    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: claimed.id,
                ok: false,
                error: Some(format!("input {secret} was rejected")),
                payload: serde_json::json!({"value": secret}),
                completed_at,
            },
        )
        .await;

    let results = bridge.recent_results(session_id, 8).await;
    assert_eq!(results.len(), 1);
    let stored = &results[0];
    assert_eq!(stored.action_id, claimed.id);
    assert!(!stored.ok);
    assert_eq!(stored.completed_at, completed_at);

    let serialized = serde_json::to_string(stored).expect("result should serialize");
    assert!(!serialized.contains(secret));
}
