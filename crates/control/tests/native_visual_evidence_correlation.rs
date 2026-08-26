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
            cwd: Some("/tmp/localview-native-visual-correlation".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Native Visual Correlation".into()),
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
        live: LiveBridge::new(64, 8),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

async fn seed_semantic(state: &ControlState, session_id: Uuid) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![ObserverEvent {
                seq: 1,
                captured_at: Utc::now(),
                kind: ObserverEventKind::SemanticSnapshot,
                reference: None,
                route: Some("http://127.0.0.1:5173/settings".into()),
                payload: serde_json::json!({"version": 1}),
            }],
        })
        .await;
}

async fn send(
    state: ControlState,
    method: Method,
    uri: String,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer test-token");
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state)
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn cycle_body() -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": 2_000,
            "text_tokens": 800,
            "image_regions": 2,
            "chromium_spawns": 0
        },
        "deep_mode": false,
        "compatibility_requested": false,
        "target": "@save",
        "revision": "rev-native-visual",
        "viewport": {
            "css_width": 1280,
            "css_height": 720,
            "device_scale_factor": 1.0
        }
    })
}

async fn post_unrelated_evidence_and_mismatched_result(state: ControlState, session_id: Uuid) {
    for _ in 0..160 {
        let (status, requests) = send(
            state.clone(),
            Method::GET,
            format!("/v1/sessions/{session_id}/native-executor"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let Some(request) = requests.as_array().and_then(|items| items.first()) else {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        };

        let captured_at = Utc::now().timestamp_millis();
        let (evidence_status, evidence) = send(
            state.clone(),
            Method::POST,
            format!("/v1/sessions/{session_id}/evidence/visual-region"),
            Some(serde_json::json!({
                "artifact_id": "lv-0123456789abcdef",
                "pixel_width": 180,
                "pixel_height": 90,
                "backend": "webview2",
                "route": "http://127.0.0.1:5173/settings",
                "viewport": {
                    "css_width": 1280,
                    "css_height": 720,
                    "device_scale_factor": 1.0
                },
                "revision": "rev-native-visual",
                "captured_at_unix_ms": captured_at,
                "target": "region",
                "region": {"x": 300.0, "y": 250.0, "width": 180.0, "height": 90.0}
            })),
        )
        .await;
        assert_eq!(evidence_status, StatusCode::OK);
        let unrelated_id = evidence["evidence_id"]
            .as_str()
            .expect("unrelated evidence id");

        let request_id = request["id"].as_str().expect("request id");
        let (result_status, _) = send(
            state,
            Method::POST,
            format!("/v1/sessions/{session_id}/native-executor/results"),
            Some(serde_json::json!({
                "request_id": request_id,
                "ok": true,
                "error": null,
                "usage": {
                    "latency_ms": 180,
                    "text_tokens": 77,
                    "image_regions": 1,
                    "chromium_spawns": 0
                },
                "payload": {
                    "mode": "regions",
                    "capture_performed": true,
                    "selected_regions": 1,
                    "evidence_ids": [format!("{unrelated_id}-mismatch")],
                    "baseline_cached": true,
                    "snapshot_version": 1
                },
                "completed_at": Utc::now()
            })),
        )
        .await;
        assert_eq!(result_status, StatusCode::NO_CONTENT);
        return;
    }

    panic!("cycle did not enqueue a native visual executor request");
}

#[tokio::test]
async fn native_visual_result_must_correlate_to_the_exact_reported_evidence_ids() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;

    let worker_state = state.clone();
    let worker = tokio::spawn(async move {
        post_unrelated_evidence_and_mismatched_result(worker_state, session_id).await;
    });

    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(cycle_body()),
    )
    .await;
    worker.await.expect("native visual worker");

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"], "native_visual_executor_failed");
    assert_eq!(
        body["reason"],
        "native visual evidence correlation failed"
    );
}
