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
use localview_live_bridge::{
    BridgeActionKind, BridgeActionResult, LiveBridge, ObserverBatch, ObserverEvent,
    ObserverEventKind,
};
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
            cwd: Some("/tmp/localview-perception-execution-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Perception Execution Test".into()),
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

async fn post_step(
    state: ControlState,
    session_id: uuid::Uuid,
    authorized: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/sessions/{session_id}/perception/step"))
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("perception step request"),
        )
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded perception step body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn raw_snapshot_payload() -> Value {
    serde_json::json!({
        "version": 9,
        "route": "http://127.0.0.1:5173/settings",
        "viewport": {"width": 1000, "height": 800},
        "semantic_tree": {
            "ref": "@root",
            "tag": "main",
            "role": "main",
            "name": null,
            "rect": {"x": 0.0, "y": 0.0, "width": 1000.0, "height": 800.0},
            "interactive": false,
            "attributes": {},
            "sourceHint": null,
            "children": [{
                "ref": "@save",
                "tag": "button",
                "role": "button",
                "name": "Save",
                "rect": {"x": 320.0, "y": 300.0, "width": 100.0, "height": 40.0},
                "interactive": true,
                "attributes": {"type": "button"},
                "sourceHint": null,
                "children": []
            }]
        }
    })
}

async fn complete_next_snapshot(state: ControlState, session_id: uuid::Uuid) {
    for _ in 0..120 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions
            .into_iter()
            .find(|action| matches!(&action.action, BridgeActionKind::Snapshot))
        {
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("perception semantic action must be inflight before completion");
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: claimed.id,
                        ok: true,
                        error: None,
                        payload: raw_snapshot_payload(),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("perception step did not enqueue the selected semantic snapshot");
}

async fn seed_semantic(state: &ControlState, session_id: uuid::Uuid) {
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
                route: Some("/settings".into()),
                payload: serde_json::json!({"version": 1}),
            }],
        })
        .await;
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
async fn perception_step_requires_auth_and_a_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        post_step(
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
        post_step(
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
async fn missing_state_executes_only_the_planner_selected_semantic_snapshot() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(executor_state, session_id).await;
    });

    let (status, body) = post_step(state, session_id, true, request_body(false, false)).await;
    executor.await.expect("semantic executor task");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["plan"]["actions"][0]["action"]["kind"],
        "semantic_snapshot"
    );
    assert_eq!(body["engine"]["tier"], "Lightweight");
    assert_eq!(body["execution"]["kind"], "semantic_snapshot");
    assert_eq!(body["execution"]["snapshot"]["version"], 9);
    assert_eq!(
        body["execution"]["snapshot"]["route"],
        "http://127.0.0.1:5173/settings"
    );
}

#[tokio::test]
async fn visual_selection_fails_closed_without_a_visual_executor_and_queues_no_page_action() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;

    let (status, body) = post_step(
        state.clone(),
        session_id,
        true,
        request_body(false, false),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "perception_executor_unavailable");
    assert_eq!(body["action_kind"], "region_capture");
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn chromium_selection_fails_closed_without_a_tier3_executor_and_queues_no_page_action() {
    let (state, session_id) = test_state().await;
    seed_semantic_and_layout(&state, session_id).await;

    let (status, body) = post_step(
        state.clone(),
        session_id,
        true,
        request_body(false, true),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "perception_executor_unavailable");
    assert_eq!(body["action_kind"], "chromium_escalation");
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn empty_plan_is_a_noop_not_an_implicit_fallback() {
    let (state, session_id) = test_state().await;
    seed_semantic_and_layout(&state, session_id).await;

    let (status, body) = post_step(
        state.clone(),
        session_id,
        true,
        request_body(true, false),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["plan"]["actions"].as_array().map(Vec::len), Some(0));
    assert!(body["engine"].is_null());
    assert!(body["execution"].is_null());
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn public_execution_request_cannot_submit_a_plan_or_escalation_authority() {
    let (state, session_id) = test_state().await;
    let mut body = request_body(false, false);
    body["plan"] = serde_json::json!({"actions": []});
    body["budget_escalation_reason"] = serde_json::json!("critical_issue");

    let (status, _) = post_step(state, session_id, true, body).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
