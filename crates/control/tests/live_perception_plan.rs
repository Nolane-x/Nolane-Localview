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
            cwd: Some("/tmp/localview-perception-plan-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Perception Plan Test".into()),
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

fn request_body(deep_mode: bool, compatibility_requested: bool) -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": 1500,
            "text_tokens": 800,
            "image_regions": 2,
            "chromium_spawns": 0
        },
        "deep_mode": deep_mode,
        "compatibility_requested": compatibility_requested,
        "target": "@save"
    })
}

async fn post_plan(
    state: ControlState,
    session_id: uuid::Uuid,
    authorized: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/sessions/{session_id}/perception/plan"))
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("perception plan request"),
        )
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded perception plan body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn seed_semantic_and_layout(state: &ControlState, session_id: uuid::Uuid) {
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
                    route: Some("/settings".into()),
                    payload: serde_json::json!({"version": 1}),
                },
                ObserverEvent {
                    seq: 2,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Layout,
                    reference: None,
                    route: Some("/settings".into()),
                    payload: serde_json::json!({"stable": true}),
                },
            ],
        })
        .await;
}

#[tokio::test]
async fn perception_plan_requires_auth_and_a_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        post_plan(
            state.clone(),
            session_id,
            false,
            request_body(false, false),
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post_plan(
            state,
            uuid::Uuid::new_v4(),
            true,
            request_body(false, false),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn missing_state_prefers_a_cheap_semantic_observation_before_visual_capture() {
    let (state, session_id) = test_state().await;
    let (status, body) = post_plan(state, session_id, true, request_body(false, false)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signals"]["insufficient_evidence"], true);
    assert_eq!(body["plan"]["actions"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        body["plan"]["actions"][0]["action"]["kind"],
        "semantic_snapshot"
    );
    assert_eq!(body["engine"]["tier"], "Lightweight");
}

#[tokio::test]
async fn explicit_compatibility_goal_can_derive_browser_specific_authority_after_cheap_state_is_known() {
    let (state, session_id) = test_state().await;
    seed_semantic_and_layout(&state, session_id).await;

    let (status, body) = post_plan(state, session_id, true, request_body(false, true)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signals"]["browser_specific_suspicion"], true);
    assert_eq!(
        body["plan"]["actions"][0]["action"]["kind"],
        "chromium_escalation"
    );
    assert_eq!(
        body["plan"]["budget_decision"]["budget_escalation_reason"],
        "browser_specific_suspicion"
    );
    assert_eq!(body["engine"]["tier"], "Chromium");
}

#[tokio::test]
async fn chromium_is_not_planned_without_browser_specific_intent_even_in_deep_mode() {
    let (state, session_id) = test_state().await;
    seed_semantic_and_layout(&state, session_id).await;

    let (status, body) = post_plan(state, session_id, true, request_body(true, false)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["signals"]["explicit_deep_mode"], true);
    assert_eq!(body["signals"]["browser_specific_suspicion"], false);
    assert_eq!(body["plan"]["actions"].as_array().map(Vec::len), Some(0));
    assert!(body["engine"].is_null());
}

#[tokio::test]
async fn public_request_cannot_supply_a_budget_escalation_reason() {
    let (state, session_id) = test_state().await;
    let mut body = request_body(false, false);
    body["budget_escalation_reason"] = serde_json::json!("critical_issue");

    let (status, _) = post_plan(state, session_id, true, body).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
