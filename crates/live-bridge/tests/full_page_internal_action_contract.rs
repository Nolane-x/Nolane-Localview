use chrono::Utc;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use serde_json::json;
use uuid::Uuid;

#[test]
fn full_page_actions_serialize_as_private_capture_kinds() {
    let token = Uuid::new_v4();
    let scroll = serde_json::to_value(BridgeActionKind::CaptureScrollTo { token, y: 123.5 })
        .expect("capture scroll serializes");
    assert_eq!(scroll["type"], "capture_scroll_to");
    assert_eq!(scroll["token"], token.to_string());
    assert_eq!(scroll["y"], 123.5);

    let probe = serde_json::to_value(BridgeActionKind::CaptureTileProbe { token })
        .expect("capture probe serializes");
    assert_eq!(probe["type"], "capture_tile_probe");
    assert_eq!(probe["token"], token.to_string());

    assert!(BridgeActionKind::CaptureScrollTo { token, y: 0.0 }.is_internal_capture_action());
    assert!(BridgeActionKind::CaptureTileProbe { token }.is_internal_capture_action());
}

#[tokio::test]
async fn viewport_and_full_page_freeze_have_server_owned_lease_modes() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let selectors = vec!["[data-localview-private]".to_string()];

    let viewport = bridge
        .enqueue_capture_freeze(session_id, selectors.clone())
        .await;
    let viewport_private = bridge
        .take_internal_capture_actions(session_id, 8)
        .await
        .into_iter()
        .find(|queued| queued.id == viewport.id)
        .expect("viewport freeze is internal");
    assert_eq!(
        viewport_private
            .private_capture
            .as_ref()
            .and_then(|private| private.visual_freeze_lease_ms),
        Some(8_000)
    );

    let full_page = bridge
        .enqueue_full_page_capture_freeze(session_id, selectors.clone())
        .await;
    assert!(bridge.take_public_actions(session_id, 8).await.is_empty());
    let full_page_private = bridge
        .take_internal_capture_actions(session_id, 8)
        .await
        .into_iter()
        .find(|queued| queued.id == full_page.id)
        .expect("full-page freeze is internal");
    let private = full_page_private
        .private_capture
        .expect("full-page freeze carries private authority");
    assert_eq!(private.mask_selectors, selectors);
    assert_eq!(private.visual_freeze_lease_ms, Some(30_000));
}

#[tokio::test]
async fn tile_probe_carries_fresh_selectors_only_in_private_envelope() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let token = Uuid::new_v4();
    let selectors = vec![
        "[data-localview-private]".to_string(),
        "input[type=\"password\"]".to_string(),
    ];

    let action = bridge
        .enqueue_capture_tile_probe(session_id, token, selectors.clone())
        .await;
    assert!(bridge.take_public_actions(session_id, 8).await.is_empty());

    let queued = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].id, action.id);
    assert!(matches!(
        queued[0].action,
        BridgeActionKind::CaptureTileProbe { token: queued_token } if queued_token == token
    ));
    let private = queued[0]
        .private_capture
        .as_ref()
        .expect("tile probe receives private selector authority");
    assert_eq!(private.mask_selectors, selectors);
    assert_eq!(private.visual_freeze_lease_ms, None);

    let wire = serde_json::to_string(&queued[0].action).unwrap();
    assert!(!wire.contains("data-localview-private"));
    assert!(!wire.contains("password"));
}

#[tokio::test]
async fn absolute_capture_scroll_never_enters_public_queue() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let token = Uuid::new_v4();

    let action = bridge
        .enqueue_action(
            session_id,
            None,
            BridgeActionKind::CaptureScrollTo { token, y: 400.25 },
        )
        .await;
    assert!(bridge.take_public_actions(session_id, 8).await.is_empty());
    let private = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(private.len(), 1);
    assert_eq!(private[0].id, action.id);
}

#[tokio::test]
async fn capture_scroll_result_storage_is_geometry_only() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let token = Uuid::new_v4();
    let action = bridge
        .enqueue_action(
            session_id,
            None,
            BridgeActionKind::CaptureScrollTo { token, y: 400.0 },
        )
        .await;
    let queued = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(queued.len(), 1);
    let claimed = bridge
        .claim_action(session_id, action.id)
        .await
        .expect("capture scroll is claimable");

    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: claimed.id,
                ok: true,
                error: None,
                payload: json!({
                    "requested_y": 400.0,
                    "actual_x": 0.0,
                    "actual_y": 400.0,
                    "document_css_width": 800.0,
                    "document_css_height": 1600.0,
                    "viewport_css_width": 800.0,
                    "viewport_css_height": 600.0,
                    "innerText": "private page text",
                    "secret": "must-not-survive"
                }),
                completed_at: Utc::now(),
            },
        )
        .await;

    let stored = bridge.recent_internal_capture_results(session_id, 8).await;
    assert_eq!(stored.len(), 1);
    assert!(stored[0].ok);
    let payload = &stored[0].payload;
    assert_eq!(payload["requested_y"], 400.0);
    assert_eq!(payload["actual_x"], 0.0);
    assert_eq!(payload["actual_y"], 400.0);
    assert_eq!(payload["document_css_width"], 800.0);
    assert_eq!(payload["document_css_height"], 1600.0);
    assert_eq!(payload["viewport_css_width"], 800.0);
    assert_eq!(payload["viewport_css_height"], 600.0);
    let encoded = payload.to_string();
    assert!(!encoded.contains("innerText"));
    assert!(!encoded.contains("private page text"));
    assert!(!encoded.contains("secret"));
    assert!(!encoded.contains("must-not-survive"));
}

#[tokio::test]
async fn tile_probe_result_storage_is_bounded_and_selector_free() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let token = Uuid::new_v4();
    let selectors = vec!["[data-localview-private]".to_string()];
    let action = bridge
        .enqueue_capture_tile_probe(session_id, token, selectors.clone())
        .await;
    let queued = bridge.take_internal_capture_actions(session_id, 8).await;
    assert_eq!(queued.len(), 1);
    let claimed = bridge
        .claim_action(session_id, action.id)
        .await
        .expect("capture tile probe is claimable");

    bridge
        .complete_action(
            &claimed,
            BridgeActionResult {
                action_id: claimed.id,
                ok: true,
                error: None,
                payload: json!({
                    "scroll_x": 0.0,
                    "scroll_y": 600.0,
                    "document_css_width": 800.0,
                    "document_css_height": 1600.0,
                    "viewport_css_width": 800.0,
                    "viewport_css_height": 600.0,
                    "masked_elements": 1,
                    "mask_rects": [{"x": 10.0, "y": 20.0, "width": 30.0, "height": 40.0}],
                    "positional_elements_scanned": 250,
                    "visible_fixed_or_sticky": false,
                    "mask_selectors": selectors,
                    "private_page_payload": "must-not-survive"
                }),
                completed_at: Utc::now(),
            },
        )
        .await;

    let stored = bridge.recent_internal_capture_results(session_id, 8).await;
    assert_eq!(stored.len(), 1);
    assert!(stored[0].ok);
    let payload = &stored[0].payload;
    assert_eq!(payload["scroll_x"], 0.0);
    assert_eq!(payload["scroll_y"], 600.0);
    assert_eq!(payload["masked_elements"], 1);
    assert_eq!(payload["mask_rects"].as_array().map(Vec::len), Some(1));
    assert_eq!(payload["positional_elements_scanned"], 250);
    assert_eq!(payload["visible_fixed_or_sticky"], false);
    let encoded = payload.to_string();
    assert!(!encoded.contains("mask_selectors"));
    assert!(!encoded.contains("data-localview-private"));
    assert!(!encoded.contains("private_page_payload"));
    assert!(!encoded.contains("must-not-survive"));
}

#[tokio::test]
async fn internal_capture_queue_remains_bounded() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let token = Uuid::new_v4();
    for y in 0..32 {
        bridge
            .enqueue_action(
                session_id,
                None,
                BridgeActionKind::CaptureScrollTo {
                    token,
                    y: y as f64,
                },
            )
            .await;
    }
    assert_eq!(bridge.take_internal_capture_actions(session_id, 64).await.len(), 8);
    assert!(bridge.take_public_actions(session_id, 64).await.is_empty());
}
