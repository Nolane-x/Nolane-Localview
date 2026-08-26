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
            cwd: Some("/tmp/localview-runtime-resource-governor".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Runtime Resource Governor".into()),
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
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
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

async fn seed_layout(state: &ControlState, session_id: Uuid) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![ObserverEvent {
                seq: 2,
                captured_at: Utc::now(),
                kind: ObserverEventKind::Layout,
                reference: None,
                route: Some("http://127.0.0.1:5173/settings".into()),
                payload: serde_json::json!({"stable": true}),
            }],
        })
        .await;
}

fn runtime_sample(memory_mb: u64, cpu_percent: f64) -> Value {
    serde_json::json!({
        "memory_mb": memory_mb,
        "cpu_percent": cpu_percent,
        "capture_storage_mb": 12,
        "network_kb_per_minute": 24
    })
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
        "revision": "rev-resource-governor",
        "viewport": {
            "css_width": 1280,
            "css_height": 720,
            "device_scale_factor": 1.0
        }
    })
}

fn chromium_plan_body() -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": 2_000,
            "text_tokens": 800,
            "image_regions": 2,
            "chromium_spawns": 0
        },
        "deep_mode": false,
        "compatibility_requested": true,
        "target": "@save"
    })
}

async fn post_runtime_sample(state: ControlState, sample: Value) -> StatusCode {
    send(
        state,
        Method::POST,
        "/v1/runtime/resources/sample".into(),
        Some(sample),
        true,
    )
    .await
    .0
}

async fn take_native_requests_until(
    state: &ControlState,
    session_id: Uuid,
    expected: usize,
) -> usize {
    let mut observed = 0usize;
    for _ in 0..160 {
        observed += state
            .live
            .take_native_executor_requests(session_id, 8)
            .await
            .len();
        if observed >= expected {
            return observed;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    observed
}

#[tokio::test]
async fn resource_sample_ingress_is_authenticated_and_bounded_to_runtime_metrics() {
    let (state, _) = test_state().await;
    let sample = runtime_sample(96, 4.0);

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/sample".into(),
            Some(sample.clone()),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/sample".into(),
            Some(sample),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let mut forbidden = runtime_sample(96, 4.0);
    forbidden["concurrent_captures"] = serde_json::json!(0);
    assert_eq!(
        send(
            state,
            Method::POST,
            "/v1/runtime/resources/sample".into(),
            Some(forbidden),
            true,
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "callers must not be able to forge governor-owned reservation counters"
    );
}

#[tokio::test]
async fn high_memory_pressure_blocks_visual_before_native_work_is_enqueued() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;

    assert_eq!(
        post_runtime_sample(state.clone(), runtime_sample(600, 4.0)).await,
        StatusCode::NO_CONTENT
    );

    let (status, body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(cycle_body()),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "resource_governor_denied");
    assert_eq!(body["work_kind"], "native_visual_capture");
    assert!(
        state
            .live
            .take_native_executor_requests(session_id, 8)
            .await
            .is_empty(),
        "denied visual work must never cross the native executor authority boundary"
    );
}

#[tokio::test]
async fn capture_reservations_enforce_concurrency_and_release_when_cycles_are_cancelled() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;
    assert_eq!(
        post_runtime_sample(state.clone(), runtime_sample(96, 4.0)).await,
        StatusCode::NO_CONTENT
    );

    let first_state = state.clone();
    let first = tokio::spawn(async move {
        send(
            first_state,
            Method::POST,
            format!("/v1/sessions/{session_id}/perception/cycle"),
            Some(cycle_body()),
            true,
        )
        .await
    });
    let second_state = state.clone();
    let second = tokio::spawn(async move {
        send(
            second_state,
            Method::POST,
            format!("/v1/sessions/{session_id}/perception/cycle"),
            Some(cycle_body()),
            true,
        )
        .await
    });

    assert_eq!(
        take_native_requests_until(&state, session_id, 2).await,
        2,
        "the first two captures should hold the default two capture reservations"
    );

    let (third_status, third_body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(cycle_body()),
        true,
    )
    .await;
    assert_eq!(third_status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(third_body["error"], "resource_governor_denied");
    assert_eq!(third_body["work_kind"], "native_visual_capture");

    first.abort();
    second.abort();
    let _ = first.await;
    let _ = second.await;

    let fourth_state = state.clone();
    let fourth = tokio::spawn(async move {
        send(
            fourth_state,
            Method::POST,
            format!("/v1/sessions/{session_id}/perception/cycle"),
            Some(cycle_body()),
            true,
        )
        .await
    });
    assert_eq!(
        take_native_requests_until(&state, session_id, 1).await,
        1,
        "cancelling the owning cycles must release their governor reservations"
    );
    fourth.abort();
    let _ = fourth.await;
}

#[tokio::test]
async fn high_runtime_pressure_blocks_chromium_before_engine_admission() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;
    seed_layout(&state, session_id).await;
    assert_eq!(
        post_runtime_sample(state.clone(), runtime_sample(96, 20.0)).await,
        StatusCode::NO_CONTENT
    );

    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/plan"),
        Some(chromium_plan_body()),
        true,
    )
    .await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "resource_governor_denied");
    assert_eq!(body["work_kind"], "chromium");
}
