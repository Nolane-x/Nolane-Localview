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
            cwd: Some(format!("/tmp/localview-cancel-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Cancellation {port}")),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered(5173), discovered(5174)], Utc::now())
        .await;
    assert_eq!(reconcile.created.len(), 2);
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, reconcile.created[0], reconcile.created[1])
}

fn visual_diff_action() -> NativeExecutorAction {
    NativeExecutorAction::VisualDiffCapture {
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.0,
        },
        revision: Some("rev-cancel".into()),
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

async fn cancel(state: ControlState, session_id: Uuid, request_id: Uuid) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/cancel"),
        Some(serde_json::json!({"request_id": request_id})),
    )
    .await
}

#[tokio::test]
async fn queued_cancel_is_terminal_idempotent_and_session_scoped() {
    let (state, owner, other) = test_state().await;
    let request = state
        .live
        .enqueue_native_executor(owner, visual_diff_action())
        .await;

    let (wrong_status, wrong_body) = cancel(state.clone(), other, request.id).await;
    assert_eq!(wrong_status, StatusCode::NOT_FOUND, "cross-session: {wrong_body}");

    let (status, body) = cancel(state.clone(), owner, request.id).await;
    assert_eq!(status, StatusCode::OK, "cancel: {body}");
    assert_eq!(body["request_id"], request.id.to_string());
    assert_eq!(body["state"], "cancelled");
    assert_eq!(body["acknowledged"], true);

    let (repeat_status, repeat_body) = cancel(state.clone(), owner, request.id).await;
    assert_eq!(repeat_status, StatusCode::OK, "repeat: {repeat_body}");
    assert_eq!(repeat_body["state"], "cancelled");
    assert_eq!(repeat_body["acknowledged"], true);

    let (poll_status, poll_body) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor"),
        None,
    )
    .await;
    assert_eq!(poll_status, StatusCode::OK);
    assert_eq!(poll_body, serde_json::json!([]));
}

#[tokio::test]
async fn inflight_cancel_emits_one_cooperative_signal_and_ack_is_idempotent() {
    let (state, owner, _) = test_state().await;
    let request = state
        .live
        .enqueue_native_executor(owner, visual_diff_action())
        .await;

    let (take_status, take_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body.as_array().map(Vec::len), Some(1));

    let (status, body) = cancel(state.clone(), owner, request.id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "cancel request: {body}");
    assert_eq!(body["request_id"], request.id.to_string());
    assert_eq!(body["state"], "cancellation_requested");
    assert_eq!(body["acknowledged"], false);

    let (repeat_status, repeat_body) = cancel(state.clone(), owner, request.id).await;
    assert_eq!(repeat_status, StatusCode::ACCEPTED, "repeat request: {repeat_body}");
    assert_eq!(repeat_body["state"], "cancellation_requested");

    let (signal_status, signal_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor/cancellations"),
        None,
    )
    .await;
    assert_eq!(signal_status, StatusCode::OK, "signals: {signal_body}");
    let signals = signal_body.as_array().expect("cancellation array");
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0]["request_id"], request.id.to_string());

    for _ in 0..2 {
        let (ack_status, ack_body) = send(
            state.clone(),
            Method::POST,
            format!(
                "/v1/sessions/{owner}/native-executor/cancellations/{}/ack",
                request.id
            ),
            None,
        )
        .await;
        assert_eq!(ack_status, StatusCode::NO_CONTENT, "ack: {ack_body}");
    }

    let (after_status, after_body) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor/cancellations"),
        None,
    )
    .await;
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(after_body, serde_json::json!([]));
}

#[tokio::test]
async fn acknowledged_cancellation_rejects_late_result_and_releases_executor_capacity() {
    let (state, owner, _) = test_state().await;
    let mut ids = Vec::new();
    for _ in 0..8 {
        ids.push(
            state
                .live
                .enqueue_native_executor(owner, visual_diff_action())
                .await
                .id,
        );
    }

    let (take_status, take_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body.as_array().map(Vec::len), Some(8));

    let request_id = ids[0];
    let (cancel_status, _) = cancel(state.clone(), owner, request_id).await;
    assert_eq!(cancel_status, StatusCode::ACCEPTED);
    let (ack_status, _) = send(
        state.clone(),
        Method::POST,
        format!(
            "/v1/sessions/{owner}/native-executor/cancellations/{request_id}/ack"
        ),
        None,
    )
    .await;
    assert_eq!(ack_status, StatusCode::NO_CONTENT);

    state
        .live
        .enqueue_native_executor(owner, visual_diff_action())
        .await;
    let (next_status, next_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/native-executor"),
        None,
    )
    .await;
    assert_eq!(next_status, StatusCode::OK);
    assert_eq!(
        next_body.as_array().map(Vec::len),
        Some(1),
        "one cancelled active origin must release exactly one executor slot"
    );

    let (late_status, late_body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{owner}/native-executor/results"),
        Some(serde_json::json!({
            "request_id": request_id,
            "ok": false,
            "error": "late native completion",
            "usage": null,
            "payload": null,
            "completed_at": Utc::now()
        })),
    )
    .await;
    assert_eq!(late_status, StatusCode::CONFLICT, "late result: {late_body}");
}
