use chrono::Utc;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn private_freeze_carries_selectors_but_stores_only_bounded_geometry() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let selectors = vec![
        "[data-localview-private]".to_string(),
        "input[type=\"password\"]".to_string(),
    ];

    let action = bridge
        .enqueue_action(
            session_id,
            None,
            BridgeActionKind::FreezeVisuals {
                mask_selectors: selectors.clone(),
            },
        )
        .await;

    let drained = bridge
        .take_internal_capture_actions(session_id, 8)
        .await;
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, action.id);
    match &drained[0].action {
        BridgeActionKind::FreezeVisuals { mask_selectors } => {
            assert_eq!(mask_selectors, &selectors);
        }
        other => panic!("expected private freeze action, got {other:?}"),
    }

    let claimed = bridge
        .claim_action(session_id, action.id)
        .await
        .expect("private freeze action must be claimable");
    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: claimed.id,
                ok: true,
                error: None,
                payload: json!({
                    "paused_animations": 3,
                    "web_animations_supported": true,
                    "viewport_css_width": 800.0,
                    "viewport_css_height": 600.0,
                    "masked_elements": 2,
                    "mask_rects": [
                        {"x": 10.0, "y": 20.0, "width": 100.0, "height": 30.0},
                        {"x": 200.0, "y": 100.0, "width": 50.0, "height": 40.0}
                    ],
                    "mask_selectors": selectors,
                    "private_page_payload": "must-not-escape"
                }),
                completed_at: Utc::now(),
            },
        )
        .await;

    let stored = bridge
        .recent_internal_capture_results(session_id, 8)
        .await;
    assert_eq!(stored.len(), 1);
    let payload = &stored[0].payload;
    assert_eq!(payload["paused_animations"], 3);
    assert_eq!(payload["web_animations_supported"], true);
    assert_eq!(payload["viewport_css_width"], 800.0);
    assert_eq!(payload["viewport_css_height"], 600.0);
    assert_eq!(payload["masked_elements"], 2);
    assert_eq!(payload["mask_rects"].as_array().map(Vec::len), Some(2));

    let encoded = payload.to_string();
    assert!(!encoded.contains("mask_selectors"));
    assert!(!encoded.contains("data-localview-private"));
    assert!(!encoded.contains("password"));
    assert!(!encoded.contains("private_page_payload"));
    assert!(!encoded.contains("must-not-escape"));
}
