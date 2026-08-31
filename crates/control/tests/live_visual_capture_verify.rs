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
use tokio::time::sleep;
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
            cwd: Some("/tmp/localview-capture-verify".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Capture Verify".into()),
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
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
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

fn viewport() -> Value {
    serde_json::json!({
        "css_width": 1280,
        "css_height": 720,
        "device_scale_factor": 1.0
    })
}

fn capture_verify_payload() -> Value {
    serde_json::json!({
        "viewport": viewport(),
        "revision": "rev-a",
        "expectation": {
            "kind": "unchanged",
            "max_changed_ratio": 0.0
        }
    })
}

async fn take_native_request(state: ControlState, session_id: Uuid) -> Value {
    for _ in 0..100 {
        let (status, body) = send(
            state.clone(),
            Method::GET,
            format!("/v1/sessions/{session_id}/native-executor"),
            None,
            true,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "native executor poll: {body}");
        if let Some(request) = body.as_array().and_then(|items| items.first()).cloned() {
            return request;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("capture+verify did not enqueue a native visual diff request");
}

#[tokio::test]
async fn capture_verify_queues_native_diff_capture_and_returns_server_verified_result() {
    let (state, session_id) = test_state().await;
    let verify_state = state.clone();
    let verify_task = tokio::spawn(async move {
        send(
            verify_state,
            Method::POST,
            format!("/v1/sessions/{session_id}/verify/visual/capture"),
            Some(capture_verify_payload()),
            true,
        )
        .await
    });

    let request = take_native_request(state.clone(), session_id).await;
    assert_eq!(request["session_id"], session_id.to_string());
    assert_eq!(request["action"]["type"], "visual_diff_capture");
    assert_eq!(request["action"]["viewport"], viewport());
    assert_eq!(request["action"]["revision"], "rev-a");
    let request_id = request["id"].as_str().expect("request id");

    let (status, diff) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual-diff"),
        Some(serde_json::json!({
            "route": "http://127.0.0.1:5173/settings",
            "viewport": viewport(),
            "revision": "rev-a",
            "captured_at_unix_ms": Utc::now().timestamp_millis(),
            "mode": "unchanged",
            "changed_ratio": 0.0,
            "visual_evidence_ids": []
        })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual diff evidence: {diff}");
    let diff_id = diff["evidence_id"].as_str().expect("diff id").to_owned();

    let (status, body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/results"),
        Some(serde_json::json!({
            "request_id": request_id,
            "ok": true,
            "error": null,
            "usage": null,
            "payload": {
                "mode": "unchanged",
                "changed_ratio": 0.0,
                "evidence_ids": [],
                "visual_diff_evidence_id": diff_id,
                "baseline_cached": true
            },
            "completed_at": Utc::now()
        })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "native result: {body}");

    let (status, body) = verify_task.await.expect("verify task");
    assert_eq!(status, StatusCode::OK, "capture verify: {body}");
    assert_eq!(body["evidence_id"], diff_id);
    assert_eq!(body["result"]["verdict"], "pass");
    assert_eq!(body["result"]["changed_ratio"], 0.0);
    assert_eq!(body["native_request_id"], request_id);
}

#[tokio::test]
async fn capture_verify_requires_auth_known_session_and_server_owned_observation() {
    let (state, session_id) = test_state().await;

    let (status, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual/capture"),
        Some(capture_verify_payload()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{}/verify/visual/capture", Uuid::new_v4()),
        Some(capture_verify_payload()),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let mut payload = capture_verify_payload();
    payload["changed_ratio"] = serde_json::json!(0.0);
    payload["verdict"] = serde_json::json!("pass");
    payload["evidence_id"] = serde_json::json!("caller-controlled");
    let (status, _) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual/capture"),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
