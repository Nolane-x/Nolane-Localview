use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use chrono::Utc;
use localview_capture::StableCapturePolicy;
use localview_control::{router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore};
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

fn discovered() -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: 5173,
                scheme: "http".into(),
            },
            pid: Some(42),
            process_name: Some("node".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/localview-full-page-control-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Full Page Control Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

async fn post(
    state: ControlState,
    uri: String,
    authorized: bool,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("POST").uri(uri);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).expect("request JSON"))
        }
        None => Body::empty(),
    };
    let response = router(state)
        .oneshot(builder.body(body).expect("full-page control request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded response body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn complete_full_page_freeze(state: ControlState, session_id: Uuid) -> Uuid {
    for _ in 0..150 {
        let actions = state
            .live
            .take_internal_capture_actions(session_id, 16)
            .await;
        if let Some(action) = actions.into_iter().find(|action| {
            matches!(action.action, BridgeActionKind::FreezeVisuals)
                && action
                    .private_capture
                    .as_ref()
                    .and_then(|private| private.visual_freeze_lease_ms)
                    == Some(30_000)
        }) {
            assert_eq!(
                action
                    .private_capture
                    .as_ref()
                    .map(|private| &private.mask_selectors),
                Some(&StableCapturePolicy::default().mask_selectors)
            );
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("full-page freeze must be inflight");
            state
                .live
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
                            "masked_elements": 1,
                            "mask_rects": [
                                {"x": 10.0, "y": 20.0, "width": 30.0, "height": 40.0}
                            ],
                            "scroll_x": 0.0,
                            "scroll_y": 120.0,
                            "document_css_width": 800.0,
                            "document_css_height": 1600.0,
                            "secret": "must-not-escape",
                            "mask_selectors": ["must-not-escape"]
                        }),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return claimed.id;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("full-page freeze action was not enqueued");
}

async fn complete_scroll(
    state: ControlState,
    session_id: Uuid,
    expected_token: Uuid,
    expected_y: f64,
) {
    for _ in 0..150 {
        let actions = state
            .live
            .take_internal_capture_actions(session_id, 16)
            .await;
        if let Some(action) = actions.into_iter().find(|action| {
            matches!(
                action.action,
                BridgeActionKind::CaptureScrollTo { token, y }
                    if token == expected_token && y == expected_y
            )
        }) {
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("capture scroll must be inflight");
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: claimed.id,
                        ok: true,
                        error: None,
                        payload: json!({
                            "requested_y": expected_y,
                            "actual_x": 0.0,
                            "actual_y": expected_y,
                            "document_css_width": 800.0,
                            "document_css_height": 1600.0,
                            "viewport_css_width": 800.0,
                            "viewport_css_height": 600.0,
                            "innerText": "private page text",
                            "secret": "must-not-escape"
                        }),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("capture scroll action was not enqueued");
}

async fn complete_probe(state: ControlState, session_id: Uuid, expected_token: Uuid) {
    for _ in 0..150 {
        let actions = state
            .live
            .take_internal_capture_actions(session_id, 16)
            .await;
        if let Some(action) = actions.into_iter().find(|action| {
            matches!(
                action.action,
                BridgeActionKind::CaptureTileProbe { token } if token == expected_token
            )
        }) {
            assert_eq!(
                action
                    .private_capture
                    .as_ref()
                    .map(|private| &private.mask_selectors),
                Some(&StableCapturePolicy::default().mask_selectors)
            );
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("capture probe must be inflight");
            state
                .live
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
                            "mask_rects": [
                                {"x": 12.0, "y": 24.0, "width": 36.0, "height": 48.0}
                            ],
                            "positional_elements_scanned": 250,
                            "visible_fixed_or_sticky": false,
                            "innerText": "private page text",
                            "secret": "must-not-escape"
                        }),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("capture tile probe action was not enqueued");
}

#[tokio::test]
async fn full_page_routes_require_auth_and_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        post(
            state.clone(),
            format!("/v1/sessions/{session_id}/capture-freeze-full-page"),
            false,
            None,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post(
            state,
            format!(
                "/v1/sessions/{}/capture-freeze-full-page",
                Uuid::new_v4()
            ),
            true,
            None,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn full_page_freeze_returns_only_bounded_extended_receipt() {
    let (state, session_id) = test_state().await;
    let executor = tokio::spawn(complete_full_page_freeze(state.clone(), session_id));
    let (status, body) = post(
        state,
        format!("/v1/sessions/{session_id}/capture-freeze-full-page"),
        true,
        None,
    )
    .await;
    let token = executor.await.expect("freeze executor");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["token"], token.to_string());
    assert_eq!(body["lease_ms"], 30_000);
    assert_eq!(body["scroll_x"], 0.0);
    assert_eq!(body["scroll_y"], 120.0);
    assert_eq!(body["document_css_width"], 800.0);
    assert_eq!(body["document_css_height"], 1600.0);
    let encoded = body.to_string();
    assert!(!encoded.contains("secret"));
    assert!(!encoded.contains("must-not-escape"));
    assert!(!encoded.contains("mask_selectors"));
}

#[tokio::test]
async fn capture_scroll_and_probe_return_exact_sanitized_receipts() {
    let (state, session_id) = test_state().await;
    let token = Uuid::new_v4();

    let scroll_executor = tokio::spawn(complete_scroll(state.clone(), session_id, token, 600.0));
    let (scroll_status, scroll) = post(
        state.clone(),
        format!("/v1/sessions/{session_id}/capture-scroll"),
        true,
        Some(json!({"token": token, "y": 600.0})),
    )
    .await;
    scroll_executor.await.expect("scroll executor");
    assert_eq!(scroll_status, StatusCode::OK);
    assert_eq!(scroll["requested_y"], 600.0);
    assert_eq!(scroll["actual_y"], 600.0);
    assert_eq!(scroll["document_css_height"], 1600.0);
    assert!(!scroll.to_string().contains("private page text"));
    assert!(!scroll.to_string().contains("secret"));

    let probe_executor = tokio::spawn(complete_probe(state.clone(), session_id, token));
    let (probe_status, probe) = post(
        state,
        format!("/v1/sessions/{session_id}/capture-tile-probe"),
        true,
        Some(json!({"token": token})),
    )
    .await;
    probe_executor.await.expect("probe executor");
    assert_eq!(probe_status, StatusCode::OK);
    assert_eq!(probe["positional_elements_scanned"], 250);
    assert_eq!(probe["visible_fixed_or_sticky"], false);
    assert_eq!(probe["mask_rects"].as_array().map(Vec::len), Some(1));
    assert!(!probe.to_string().contains("private page text"));
    assert!(!probe.to_string().contains("secret"));
}

#[tokio::test]
async fn full_page_requests_deny_unknown_fields_and_out_of_range_scroll() {
    let (state, session_id) = test_state().await;
    let token = Uuid::new_v4();
    let (unknown_status, _) = post(
        state.clone(),
        format!("/v1/sessions/{session_id}/capture-scroll"),
        true,
        Some(json!({"token": token, "y": 0.0, "lease_ms": 99_999})),
    )
    .await;
    assert_eq!(unknown_status, StatusCode::UNPROCESSABLE_ENTITY);

    let (range_status, body) = post(
        state,
        format!("/v1/sessions/{session_id}/capture-scroll"),
        true,
        Some(json!({"token": token, "y": 50_001.0})),
    )
    .await;
    assert_eq!(range_status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_capture_scroll_request");
}

#[tokio::test]
async fn public_action_route_rejects_new_internal_capture_actions() {
    let (state, session_id) = test_state().await;
    let token = Uuid::new_v4();
    for action in [
        json!({"type": "capture_scroll_to", "token": token, "y": 100.0}),
        json!({"type": "capture_tile_probe", "token": token}),
    ] {
        let (status, body) = post(
            state.clone(),
            format!("/v1/sessions/{session_id}/actions"),
            true,
            Some(json!({"reference": null, "action": action})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "internal_capture_action_not_public");
    }
}

#[tokio::test]
async fn internal_capture_completion_never_creates_public_interaction_evidence() {
    let (state, session_id) = test_state().await;
    let token = Uuid::new_v4();
    let queued = state
        .live
        .enqueue_action(
            session_id,
            None,
            BridgeActionKind::CaptureScrollTo { token, y: 100.0 },
        )
        .await;
    let drained = state
        .live
        .take_internal_capture_actions(session_id, 8)
        .await;
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].id, queued.id);

    let (status, _) = post(
        state.clone(),
        format!("/v1/sessions/{session_id}/actions/results"),
        true,
        Some(json!({
            "action_id": queued.id,
            "ok": true,
            "error": null,
            "payload": {
                "requested_y": 100.0,
                "actual_x": 0.0,
                "actual_y": 100.0,
                "document_css_width": 800.0,
                "document_css_height": 1600.0,
                "viewport_css_width": 800.0,
                "viewport_css_height": 600.0,
                "secret": "must-not-escape"
            },
            "completed_at": Utc::now()
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(
        state
            .evidence
            .recent_for_session(session_id, 64)
            .await
            .is_empty(),
        "internal capture transport acknowledgements must never become public interaction evidence"
    );
    let stored = state
        .live
        .recent_internal_capture_results(session_id, 8)
        .await;
    assert_eq!(stored.len(), 1);
    assert!(stored[0].ok);
    assert!(!stored[0].payload.to_string().contains("secret"));
}


fn valid_full_page_evidence() -> Value {
    json!({
        "artifact_id": "lv-0123456789abcdef",
        "pixel_width": 800,
        "pixel_height": 1600,
        "backend": "webview2",
        "route": "http://127.0.0.1:5173/page?token=must-not-survive#fragment",
        "viewport": {
            "css_width": 800,
            "css_height": 600,
            "device_scale_factor": 1.0
        },
        "revision": "abc123",
        "captured_at_unix_ms": 123,
        "document_css_width": 800.0,
        "document_css_height": 1600.0,
        "tile_count": 3,
        "scroll_offsets_y": [0.0, 600.0, 1000.0]
    })
}

#[tokio::test]
async fn full_page_visual_evidence_is_strict_canonical_and_content_free() {
    let (state, session_id) = test_state().await;
    let (status, body) = post(
        state.clone(),
        format!("/v1/sessions/{session_id}/evidence/visual-full-page"),
        true,
        Some(valid_full_page_evidence()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["evidence_id"].as_str().is_some());
    assert!(body["deduplicated"].as_bool().is_some());

    let recent = state.evidence.recent_for_session(session_id, 8).await;
    assert_eq!(recent.len(), 1);
    let visual = &recent[0];
    assert_eq!(visual.kind, EvidenceKind::Visual);
    assert_eq!(visual.region.as_deref(), Some("full_page"));
    assert_eq!(visual.provenance.source, "native-capture");
    assert_eq!(visual.payload["route"], "http://127.0.0.1:5173/page");
    assert_eq!(visual.payload["tile_count"], 3);
    assert_eq!(visual.payload["scroll_offsets_y"], json!([0.0, 600.0, 1000.0]));
    let stored = visual.payload.to_string();
    assert!(!stored.contains("must-not-survive"));
    assert!(!stored.contains("token"));
    assert!(!stored.contains("mask"));
    assert!(!stored.contains("png"));
}

#[tokio::test]
async fn full_page_visual_evidence_rejects_spoofed_geometry_and_offsets() {
    let (state, session_id) = test_state().await;

    let mut cases = Vec::new();

    let mut bad_id = valid_full_page_evidence();
    bad_id["artifact_id"] = json!("arbitrary-id");
    cases.push(bad_id);

    let mut bad_backend = valid_full_page_evidence();
    bad_backend["backend"] = json!("chromium");
    cases.push(bad_backend);

    let mut bad_route = valid_full_page_evidence();
    bad_route["route"] = json!("https://example.com/");
    cases.push(bad_route);

    let mut zero_pixels = valid_full_page_evidence();
    zero_pixels["pixel_width"] = json!(0);
    cases.push(zero_pixels);

    let mut width_mismatch = valid_full_page_evidence();
    width_mismatch["document_css_width"] = json!(801.0);
    cases.push(width_mismatch);

    let mut too_tall = valid_full_page_evidence();
    too_tall["document_css_height"] = json!(50_001.0);
    cases.push(too_tall);

    let mut zero_tiles = valid_full_page_evidence();
    zero_tiles["tile_count"] = json!(0);
    zero_tiles["scroll_offsets_y"] = json!([]);
    cases.push(zero_tiles);

    let mut length_mismatch = valid_full_page_evidence();
    length_mismatch["scroll_offsets_y"] = json!([0.0, 1000.0]);
    cases.push(length_mismatch);

    let mut duplicate = valid_full_page_evidence();
    duplicate["scroll_offsets_y"] = json!([0.0, 600.0, 600.0]);
    cases.push(duplicate);

    let mut nonzero_first = valid_full_page_evidence();
    nonzero_first["scroll_offsets_y"] = json!([1.0, 600.0, 1000.0]);
    cases.push(nonzero_first);

    let mut skipped_middle = valid_full_page_evidence();
    skipped_middle["scroll_offsets_y"] = json!([0.0, 700.0, 1000.0]);
    cases.push(skipped_middle);

    let mut impossible_final = valid_full_page_evidence();
    impossible_final["scroll_offsets_y"] = json!([0.0, 600.0, 999.0]);
    cases.push(impossible_final);

    let mut too_many = valid_full_page_evidence();
    too_many["tile_count"] = json!(33);
    too_many["scroll_offsets_y"] = Value::Array(
        (0..33)
            .map(|index| json!(index as f64 * 10.0))
            .collect(),
    );
    cases.push(too_many);

    for payload in cases {
        let (status, _) = post(
            state.clone(),
            format!("/v1/sessions/{session_id}/evidence/visual-full-page"),
            true,
            Some(payload),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    assert!(state.evidence.recent_for_session(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn full_page_visual_evidence_requires_auth_session_and_exact_schema() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        post(
            state.clone(),
            format!("/v1/sessions/{session_id}/evidence/visual-full-page"),
            false,
            Some(valid_full_page_evidence()),
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );

    assert_eq!(
        post(
            state.clone(),
            format!(
                "/v1/sessions/{}/evidence/visual-full-page",
                Uuid::new_v4()
            ),
            true,
            Some(valid_full_page_evidence()),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let mut unknown = valid_full_page_evidence();
    unknown["freeze_token"] = json!("must-not-be-accepted");
    assert_eq!(
        post(
            state,
            format!("/v1/sessions/{session_id}/evidence/visual-full-page"),
            true,
            Some(unknown),
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
