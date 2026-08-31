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
use localview_live_bridge::{BridgeActionKind, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
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
            cwd: Some(format!("/tmp/localview-action-cancel-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Action cancellation {port}")),
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

async fn send(
    state: ControlState,
    method: Method,
    uri: String,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send_with_auth(state, method, uri, body, true).await
}

async fn send_with_auth(
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

async fn cancel(state: ControlState, session_id: Uuid, action_id: Uuid) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/actions/cancel"),
        Some(serde_json::json!({"action_id": action_id})),
    )
    .await
}

#[tokio::test]
async fn action_cancellation_routes_require_auth_and_known_session() {
    let (state, owner, _) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, None, BridgeActionKind::Click)
        .await;

    let (unauthorized, _) = send_with_auth(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/actions/cancel"),
        Some(serde_json::json!({"action_id": action.id})),
        false,
    )
    .await;
    assert_eq!(unauthorized, StatusCode::UNAUTHORIZED);

    let unknown_session = Uuid::new_v4();
    let (missing, body) = cancel(state, unknown_session, action.id).await;
    assert_eq!(missing, StatusCode::NOT_FOUND, "unknown session: {body}");
    assert_eq!(body["error"], "session_not_found");
}

#[tokio::test]
async fn queued_cancel_is_terminal_idempotent_and_session_scoped() {
    let (state, owner, other) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, Some("@save".into()), BridgeActionKind::Click)
        .await;

    let (wrong_status, wrong_body) = cancel(state.clone(), other, action.id).await;
    assert_eq!(wrong_status, StatusCode::NOT_FOUND, "cross-session: {wrong_body}");
    assert_eq!(wrong_body["error"], "action_not_found");

    let (status, body) = cancel(state.clone(), owner, action.id).await;
    assert_eq!(status, StatusCode::OK, "cancel: {body}");
    assert_eq!(body["action_id"], action.id.to_string());
    assert_eq!(body["state"], "cancelled");
    assert_eq!(body["acknowledged"], true);

    let (repeat_status, repeat_body) = cancel(state.clone(), owner, action.id).await;
    assert_eq!(repeat_status, StatusCode::OK, "repeat: {repeat_body}");
    assert_eq!(repeat_body["state"], "cancelled");

    let (take_status, take_body) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/actions"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body, serde_json::json!([]));
}

#[tokio::test]
async fn inflight_cancel_exposes_exact_signal_and_acknowledges_cooperatively() {
    let (state, owner, _) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, None, BridgeActionKind::Focus)
        .await;

    let (take_status, take_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body.as_array().map(Vec::len), Some(1));

    let (status, body) = cancel(state.clone(), owner, action.id).await;
    assert_eq!(status, StatusCode::ACCEPTED, "cancel request: {body}");
    assert_eq!(body["state"], "cancellation_requested");
    assert_eq!(body["acknowledged"], false);

    let (exact_status, exact_body) = send(
        state.clone(),
        Method::GET,
        format!(
            "/v1/sessions/{owner}/actions/cancellations/{}",
            action.id
        ),
        None,
    )
    .await;
    assert_eq!(exact_status, StatusCode::OK, "exact signal: {exact_body}");
    assert_eq!(exact_body["action_id"], action.id.to_string());

    let (list_status, list_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions/cancellations"),
        None,
    )
    .await;
    assert_eq!(list_status, StatusCode::OK);
    assert_eq!(list_body.as_array().map(Vec::len), Some(1));

    for _ in 0..2 {
        let (ack_status, ack_body) = send(
            state.clone(),
            Method::POST,
            format!(
                "/v1/sessions/{owner}/actions/cancellations/{}/ack",
                action.id
            ),
            None,
        )
        .await;
        assert_eq!(ack_status, StatusCode::NO_CONTENT, "ack: {ack_body}");
    }

    let (after_status, after_body) = send(
        state,
        Method::GET,
        format!(
            "/v1/sessions/{owner}/actions/cancellations/{}",
            action.id
        ),
        None,
    )
    .await;
    assert_eq!(after_status, StatusCode::NO_CONTENT);
    assert_eq!(after_body, Value::Null);
}

#[tokio::test]
async fn cancelled_result_is_fenced_before_any_action_evidence_is_inserted() {
    let (state, owner, _) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, None, BridgeActionKind::Snapshot)
        .await;
    let (take_status, _) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    let evidence_before = state.evidence.recent_for_session(owner, 128).await.len();

    let (cancel_status, _) = cancel(state.clone(), owner, action.id).await;
    assert_eq!(cancel_status, StatusCode::ACCEPTED);

    let (late_status, late_body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/actions/results"),
        Some(serde_json::json!({
            "action_id": action.id,
            "ok": true,
            "error": null,
            "payload": {"semantic": "must-not-be-retained"},
            "completed_at": Utc::now()
        })),
    )
    .await;
    assert_eq!(late_status, StatusCode::CONFLICT, "late result: {late_body}");
    assert_eq!(late_body["error"], "action_result_without_inflight_origin");
    assert_eq!(
        state.evidence.recent_for_session(owner, 128).await.len(),
        evidence_before,
        "cancelled result must not create Interaction/Semantic/Layout evidence"
    );
}

#[tokio::test]
async fn public_cancellation_cannot_address_internal_capture_actions() {
    let (state, owner, _) = test_state().await;
    let freeze = state
        .live
        .enqueue_capture_freeze(owner, vec![".secret".into()])
        .await;

    let (status, body) = cancel(state.clone(), owner, freeze.id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "internal cancel: {body}");
    assert_eq!(body["error"], "action_not_found");

    let internal = state.live.take_internal_capture_actions(owner, 8).await;
    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].id, freeze.id);
}
