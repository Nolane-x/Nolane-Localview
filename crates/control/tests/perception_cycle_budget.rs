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
            cwd: Some("/tmp/localview-perception-cycle-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Perception Cycle Test".into()),
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

fn cycle_body(latency_ms: u64) -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": latency_ms,
            "text_tokens": 800,
            "image_regions": 2,
            "chromium_spawns": 0
        },
        "deep_mode": false,
        "compatibility_requested": false,
        "target": "@save"
    })
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

async fn post_json(
    state: ControlState,
    uri: String,
    body: Value,
) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::AUTHORIZATION, "Bearer test-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("control request"),
        )
        .await
        .expect("control response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded control response body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn post_cycle(
    state: ControlState,
    session_id: uuid::Uuid,
    body: Value,
) -> (StatusCode, Value) {
    post_json(
        state,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        body,
    )
    .await
}

async fn complete_next_snapshot_through_control(
    state: ControlState,
    session_id: uuid::Uuid,
    completion_delay: Duration,
) {
    for _ in 0..120 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions
            .into_iter()
            .find(|action| matches!(&action.action, BridgeActionKind::Snapshot))
        {
            if !completion_delay.is_zero() {
                tokio::time::sleep(completion_delay).await;
            }
            let result = BridgeActionResult {
                action_id: action.id,
                ok: true,
                error: None,
                payload: raw_snapshot_payload(),
                completed_at: Utc::now(),
            };
            let (status, _) = post_json(
                state,
                format!("/v1/sessions/{session_id}/actions/results"),
                serde_json::to_value(result).expect("snapshot result json"),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("perception cycle did not enqueue the selected semantic snapshot");
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

#[tokio::test]
async fn public_cycle_request_cannot_submit_spent_plan_or_escalation_authority() {
    let (state, session_id) = test_state().await;
    let mut body = cycle_body(1_500);
    body["spent"] = serde_json::json!({
        "latency_ms": 0,
        "text_tokens": 0,
        "image_regions": 0,
        "chromium_spawns": 0
    });
    body["plan"] = serde_json::json!({"actions": []});
    body["budget_escalation_reason"] = serde_json::json!("critical_issue");

    let (status, _) = post_cycle(state, session_id, body).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn semantic_cycle_executes_retains_and_replans_to_noop_with_cumulative_usage() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot_through_control(executor_state, session_id, Duration::ZERO).await;
    });

    let (status, body) = post_cycle(state.clone(), session_id, cycle_body(1_500)).await;
    executor.await.expect("semantic executor task");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["completion"], "no_op");
    assert_eq!(body["steps"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        body["steps"][0]["plan"]["actions"][0]["action"]["kind"],
        "semantic_snapshot"
    );
    assert_eq!(body["steps"][0]["execution"]["kind"], "semantic_snapshot");
    assert_eq!(body["usage"]["text_tokens"], 120);
    assert_eq!(body["usage"]["image_regions"], 0);
    assert_eq!(body["usage"]["chromium_spawns"], 0);
    assert!(body["usage"]["latency_ms"].as_u64().is_some());
    assert_eq!(body["budget_decision"]["usage"], body["usage"]);
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn post_execution_latency_overrun_is_rechecked_with_planner_owned_reason() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot_through_control(
            executor_state,
            session_id,
            Duration::from_millis(10),
        )
        .await;
    });

    let (status, body) = post_cycle(state, session_id, cycle_body(1)).await;
    executor.await.expect("semantic executor task");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["completion"], "no_op");
    assert_eq!(body["budget_decision"]["status"], "escalated");
    assert_eq!(
        body["budget_decision"]["budget_escalation_reason"],
        "insufficient_evidence"
    );
    assert!(
        body["budget_decision"]["exceeded"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "latency_ms"))
    );
    assert!(body["usage"]["latency_ms"].as_u64().is_some_and(|value| value > 1));

    let step_decision = &body["steps"][0]["post_execution_budget_decision"];
    assert_eq!(step_decision["status"], body["budget_decision"]["status"]);
    assert_eq!(step_decision["budget"], body["budget_decision"]["budget"]);
    assert_eq!(
        step_decision["budget_escalation_reason"],
        body["budget_decision"]["budget_escalation_reason"]
    );
    assert_eq!(step_decision["exceeded"], body["budget_decision"]["exceeded"]);
    assert_eq!(step_decision["usage"]["text_tokens"], body["usage"]["text_tokens"]);
    assert_eq!(step_decision["usage"]["image_regions"], body["usage"]["image_regions"]);
    assert_eq!(
        step_decision["usage"]["chromium_spawns"],
        body["usage"]["chromium_spawns"]
    );
    let step_latency = step_decision["usage"]["latency_ms"]
        .as_u64()
        .expect("post-execution latency");
    let final_latency = body["usage"]["latency_ms"]
        .as_u64()
        .expect("completion latency");
    assert!(final_latency >= step_latency);
}

#[tokio::test]
async fn selected_visual_action_without_viewport_fails_closed_and_queues_nothing() {
    let (state, session_id) = test_state().await;
    seed_semantic(&state, session_id).await;

    let (status, body) = post_cycle(state.clone(), session_id, cycle_body(1_500)).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"], "perception_visual_viewport_required");
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
    assert!(
        state
            .live
            .take_native_executor_requests(session_id, 8)
            .await
            .is_empty()
    );
}
