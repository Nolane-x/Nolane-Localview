#![recursion_limit = "256"]

use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::Value;
use tower::ServiceExt;

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
            cwd: Some("/tmp/localview-chromium-rendered-plan".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Rendered Planning".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, uuid::Uuid) {
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

async fn seed_known_state(state: &ControlState, session_id: uuid::Uuid) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![
                ObserverEvent {
                    seq: 1,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::SemanticSnapshot,
                    reference: None,
                    route: Some("/settings?token=secret#panel".into()),
                    payload: serde_json::json!({"version": 1}),
                },
                ObserverEvent {
                    seq: 2,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Layout,
                    reference: None,
                    route: Some("/settings?token=secret#panel".into()),
                    payload: serde_json::json!({"stable": true}),
                },
            ],
        })
        .await;
}

fn request_body(image_regions: usize, with_viewport: bool) -> Value {
    let mut body = serde_json::json!({
        "budget": {
            "latency_ms": 1500,
            "text_tokens": 800,
            "image_regions": image_regions,
            "chromium_spawns": 0
        },
        "compatibility_requested": true,
        "target": "@save",
        "revision": "rev-rendered"
    });
    if with_viewport {
        body["viewport"] = serde_json::json!({
            "css_width": 1280,
            "css_height": 720,
            "device_scale_factor": 2.0
        });
    }
    body
}

async fn post_plan(state: ControlState, session_id: uuid::Uuid, body: Value) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v1/sessions/{session_id}/perception/plan"))
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::from(body.to_string()))
                .expect("rendered planning request"),
        )
        .await
        .expect("control response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

#[tokio::test]
async fn browser_suspicion_with_viewport_and_image_budget_prefers_one_rendered_chromium_action() {
    let (state, session_id) = test_state().await;
    seed_known_state(&state, session_id).await;

    let (status, body) = post_plan(state, session_id, request_body(1, true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signals"]["browser_specific_suspicion"], true);
    assert_eq!(body["plan"]["actions"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        body["plan"]["actions"][0]["action"]["kind"],
        "chromium_rendered_capture"
    );
    assert_eq!(body["plan"]["budget_decision"]["usage"]["chromium_spawns"], 1);
    assert_eq!(body["plan"]["budget_decision"]["usage"]["image_regions"], 1);
    assert_eq!(body["engine"]["tier"], "Chromium");
}

#[tokio::test]
async fn zero_image_budget_or_missing_viewport_preserves_compatibility_probe() {
    for body in [request_body(0, true), request_body(1, false)] {
        let (state, session_id) = test_state().await;
        seed_known_state(&state, session_id).await;
        let (status, response) = post_plan(state, session_id, body).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            response["plan"]["actions"][0]["action"]["kind"],
            "chromium_escalation"
        );
        assert_eq!(response["plan"]["budget_decision"]["usage"]["image_regions"], 0);
        assert_eq!(response["plan"]["budget_decision"]["usage"]["chromium_spawns"], 1);
    }
}
