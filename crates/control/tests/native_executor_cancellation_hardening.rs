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
use localview_live_bridge::{LiveBridge, NativeExecutorAction};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind, ViewportMeta,
};
use localview_sessions::SessionManager;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn discovered(port: u16) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: "http".into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("node".into()),
            command: Some("vite".into()),
            cwd: Some(format!("/tmp/localview-cancel-hardening-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Cancellation Hardening {port}")),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered(5273)], Utc::now())
        .await;
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

fn action() -> NativeExecutorAction {
    NativeExecutorAction::VisualDiffCapture {
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.0,
        },
        revision: Some("cancel-hardening".into()),
    }
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

#[tokio::test]
async fn exact_cancellation_lookup_is_not_truncated_by_signal_batching() {
    let (state, session_id) = test_state().await;
    let mut requests = Vec::new();
    for _ in 0..40 {
        requests.push(state.live.enqueue_native_executor(session_id, action()).await);
    }

    for _ in 0..5 {
        let (status, body) = send(
            state.clone(),
            Method::GET,
            format!("/v1/sessions/{session_id}/native-executor"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "dispatch: {body}");
        assert_eq!(body.as_array().map(Vec::len), Some(8));
    }

    for request in &requests {
        let (status, body) = send(
            state.clone(),
            Method::POST,
            format!("/v1/sessions/{session_id}/native-executor/cancel"),
            Some(serde_json::json!({"request_id": request.id})),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "cancel: {body}");
    }

    let target = requests.last().expect("target");
    let (status, body) = send(
        state,
        Method::GET,
        format!(
            "/v1/sessions/{session_id}/native-executor/cancellations/{}",
            target.id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "exact cancellation lookup: {body}");
    assert_eq!(body["request_id"], target.id.to_string());
}

#[tokio::test]
async fn accepted_cancellation_fences_result_before_acknowledgement() {
    let (state, session_id) = test_state().await;
    let request = state.live.enqueue_native_executor(session_id, action()).await;

    let (dispatch_status, dispatch_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{session_id}/native-executor"),
        None,
    )
    .await;
    assert_eq!(dispatch_status, StatusCode::OK, "dispatch: {dispatch_body}");

    let (cancel_status, cancel_body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/cancel"),
        Some(serde_json::json!({"request_id": request.id})),
    )
    .await;
    assert_eq!(cancel_status, StatusCode::ACCEPTED, "cancel: {cancel_body}");

    let (result_status, result_body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/results"),
        Some(serde_json::json!({
            "request_id": request.id,
            "ok": false,
            "error": "native result raced cancellation acknowledgement",
            "usage": null,
            "payload": null,
            "completed_at": Utc::now()
        })),
    )
    .await;
    assert_eq!(
        result_status,
        StatusCode::CONFLICT,
        "accepted cancellation must fence a racing native result before ACK: {result_body}"
    );
}
