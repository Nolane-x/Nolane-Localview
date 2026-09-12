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
use localview_live_bridge::LiveBridge;
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
            cwd: Some("/tmp/localview-capture-verify-governor".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Capture Verify Governor".into()),
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
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn capture_verify_payload() -> Value {
    serde_json::json!({
        "viewport": {
            "css_width": 1280,
            "css_height": 720,
            "device_scale_factor": 1.0
        },
        "revision": "rev-governor",
        "expectation": {
            "kind": "unchanged",
            "max_changed_ratio": 0.0
        }
    })
}

#[tokio::test]
async fn high_memory_pressure_blocks_capture_verify_before_native_work_is_enqueued() {
    let (state, session_id) = test_state().await;

    let (sample_status, _) = send(
        state.clone(),
        Method::POST,
        "/v1/runtime/resources/sample".into(),
        Some(serde_json::json!({
            "memory_mb": 600,
            "cpu_percent": 4.0,
            "capture_storage_mb": 12,
            "network_kb_per_minute": 24
        })),
    )
    .await;
    assert_eq!(sample_status, StatusCode::NO_CONTENT);

    let (status, body) = tokio::time::timeout(
        Duration::from_millis(250),
        send(
            state.clone(),
            Method::POST,
            format!("/v1/sessions/{session_id}/verify/visual/capture"),
            Some(capture_verify_payload()),
        ),
    )
    .await
    .expect("resource governor denial must happen before native result waiting");

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "denial: {body}");
    assert_eq!(body["error"], "resource_governor_denied");
    assert_eq!(body["work_kind"], "native_visual_capture");
    assert!(
        state
            .live
            .take_native_executor_requests(session_id, 8)
            .await
            .is_empty(),
        "denied capture+verify work must never cross the native executor authority boundary"
    );
}
