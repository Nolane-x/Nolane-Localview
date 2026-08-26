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
use localview_live_bridge::{LiveBridge, NativeExecutorAction, NativeExecutorResult};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind, ViewportMeta,
};
use localview_sessions::SessionManager;
use localview_token_budget::{
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetUsage,
};
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
            cwd: Some("/tmp/localview-native-executor-transport".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Native Executor Transport".into()),
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
        live: LiveBridge::new(32, 8),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

fn native_action() -> NativeExecutorAction {
    NativeExecutorAction::VisualPacket {
        reference: Some("@save".into()),
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.0,
        },
        revision: Some("rev-transport".into()),
        budget: PerceptionBudgetContract {
            latency_ms: 1_500,
            text_tokens: 400,
            image_regions: 1,
            chromium_spawns: 0,
        },
        budget_escalation_reason: Some(BudgetEscalationReason::InsufficientEvidence),
    }
}

fn native_result(request_id: Uuid) -> NativeExecutorResult {
    NativeExecutorResult {
        request_id,
        ok: true,
        error: None,
        usage: Some(PerceptionBudgetUsage {
            latency_ms: 180,
            text_tokens: 91,
            image_regions: 1,
            chromium_spawns: 0,
        }),
        payload: serde_json::json!({"selection_mode":"regions","receipt_count":1}),
        completed_at: Utc::now(),
    }
}

async fn request(
    state: ControlState,
    method: Method,
    uri: String,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
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
        .expect("bounded response body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn native_executor_poll_requires_auth_and_known_session() {
    let (state, session_id) = test_state().await;
    state
        .live
        .enqueue_native_executor(session_id, native_action())
        .await;

    let (unauthorized, _) = request(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{session_id}/native-executor"),
        None,
        None,
    )
    .await;
    assert_eq!(unauthorized, StatusCode::UNAUTHORIZED);

    let (unknown, _) = request(
        state,
        Method::GET,
        format!("/v1/sessions/{}/native-executor", Uuid::new_v4()),
        Some("test-token"),
        None,
    )
    .await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn transport_has_no_public_native_request_creation_route() {
    let (state, session_id) = test_state().await;
    let (status, _) = request(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor"),
        Some("test-token"),
        Some(serde_json::to_value(native_action()).expect("native action json")),
    )
    .await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn native_result_requires_exact_taken_origin_before_completion() {
    let (state, session_id) = test_state().await;
    let queued = state
        .live
        .enqueue_native_executor(session_id, native_action())
        .await;

    let (before_take, body) = request(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/results"),
        Some("test-token"),
        Some(serde_json::to_value(native_result(queued.id)).expect("result json")),
    )
    .await;
    assert_eq!(before_take, StatusCode::CONFLICT);
    assert_eq!(body["error"], "native_executor_result_without_inflight_origin");

    let (take_status, taken) = request(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{session_id}/native-executor"),
        Some("test-token"),
        None,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(taken.as_array().map(Vec::len), Some(1));
    assert_eq!(taken[0]["id"], queued.id.to_string());

    let random = Uuid::new_v4();
    let (wrong_origin, _) = request(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/results"),
        Some("test-token"),
        Some(serde_json::to_value(native_result(random)).expect("wrong result json")),
    )
    .await;
    assert_eq!(wrong_origin, StatusCode::CONFLICT);

    let (complete_status, _) = request(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/native-executor/results"),
        Some("test-token"),
        Some(serde_json::to_value(native_result(queued.id)).expect("result json")),
    )
    .await;
    assert_eq!(complete_status, StatusCode::NO_CONTENT);

    let results = state
        .live
        .recent_native_executor_results(session_id, 8)
        .await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].request_id, queued.id);
}

#[test]
fn native_executor_poll_expires_stale_active_authority_before_taking_more_work() {
    let source = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/native_executor.rs"),
    )
    .expect("native executor transport source");
    let expire = source
        .find("expire_native_executor_active_before")
        .expect("poll transport must expire stale native executor authority");
    let take = source
        .find("take_native_executor_requests")
        .expect("poll transport must take native executor requests");

    assert!(expire < take, "stale active origins must be expired before taking more work");
    assert!(source.contains("NATIVE_EXECUTOR_ACTIVE_LEASE_SECS"));
    assert!(source.contains("chrono::Duration::seconds(NATIVE_EXECUTOR_ACTIVE_LEASE_SECS)"));
}
