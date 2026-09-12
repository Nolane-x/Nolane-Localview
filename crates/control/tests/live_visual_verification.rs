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
            cwd: Some("/tmp/localview-live-visual-verify".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Live Visual Verify".into()),
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

async fn create_visual_parent(state: ControlState, session_id: Uuid, revision: &str) -> String {
    let payload = serde_json::json!({
        "artifact_id": "lv-0123456789abcdef",
        "pixel_width": 1280,
        "pixel_height": 720,
        "backend": "webview2",
        "route": "http://127.0.0.1:5173/settings",
        "viewport": viewport(),
        "revision": revision,
        "captured_at_unix_ms": Utc::now().timestamp_millis(),
        "target": "viewport"
    });
    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual"),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual evidence: {body}");
    body["evidence_id"]
        .as_str()
        .expect("visual evidence id")
        .to_owned()
}

async fn create_diff(
    state: ControlState,
    session_id: Uuid,
    mode: &str,
    changed_ratio: f64,
    revision: &str,
    parents: Vec<String>,
) -> String {
    let payload = serde_json::json!({
        "route": "http://127.0.0.1:5173/settings",
        "viewport": viewport(),
        "revision": revision,
        "captured_at_unix_ms": Utc::now().timestamp_millis(),
        "mode": mode,
        "changed_ratio": changed_ratio,
        "visual_evidence_ids": parents
    });
    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual-diff"),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual diff: {body}");
    body["evidence_id"]
        .as_str()
        .expect("diff evidence id")
        .to_owned()
}

fn unchanged_expectation(max_changed_ratio: f64) -> Value {
    serde_json::json!({
        "kind": "unchanged",
        "max_changed_ratio": max_changed_ratio
    })
}

#[tokio::test]
async fn live_visual_verify_requires_auth_and_a_known_session() {
    let (state, session_id) = test_state().await;
    let diff_id = create_diff(
        state.clone(),
        session_id,
        "unchanged",
        0.0,
        "rev-a",
        vec![],
    )
    .await;
    let payload = serde_json::json!({
        "evidence_id": diff_id,
        "expectation": unchanged_expectation(0.001)
    });

    let (status, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual"),
        Some(payload.clone()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{}/verify/visual", Uuid::new_v4()),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn live_visual_verify_uses_retained_diff_observation_and_explicit_policy() {
    let (state, session_id) = test_state().await;
    let diff_id = create_diff(
        state.clone(),
        session_id,
        "unchanged",
        0.0,
        "rev-a",
        vec![],
    )
    .await;

    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual"),
        Some(serde_json::json!({
            "evidence_id": diff_id,
            "expectation": unchanged_expectation(0.0)
        })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual verify: {body}");
    assert_eq!(body["result"]["verdict"], "pass");
    assert_eq!(body["result"]["changed_ratio"], 0.0);
    assert_eq!(body["evidence_id"], diff_id);
}

#[tokio::test]
async fn baseline_reset_is_live_inconclusive_not_a_false_visual_pass() {
    let (state, session_id) = test_state().await;
    let parent = create_visual_parent(state.clone(), session_id, "rev-a").await;
    let diff_id = create_diff(
        state.clone(),
        session_id,
        "baseline_reset",
        1.0,
        "rev-a",
        vec![parent],
    )
    .await;

    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual"),
        Some(serde_json::json!({
            "evidence_id": diff_id,
            "expectation": unchanged_expectation(1.0)
        })),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual verify: {body}");
    assert_eq!(body["result"]["verdict"], "inconclusive");
    assert_eq!(body["result"]["changed_ratio"], 1.0);
}

#[tokio::test]
async fn caller_cannot_submit_visual_verdict_or_failure_authority() {
    let (state, session_id) = test_state().await;
    let diff_id = create_diff(
        state.clone(),
        session_id,
        "unchanged",
        0.0,
        "rev-a",
        vec![],
    )
    .await;
    let payload = serde_json::json!({
        "evidence_id": diff_id,
        "expectation": unchanged_expectation(0.001),
        "verdict": "pass",
        "deterministic_failures": 0
    });

    let (status, _) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/verify/visual"),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
