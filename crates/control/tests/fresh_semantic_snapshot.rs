use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
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
            cwd: Some("/tmp/localview-fresh-snapshot-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Fresh Snapshot Test".into()),
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

fn raw_snapshot_payload() -> Value {
    serde_json::json!({
        "version": 9,
        "route": "http://127.0.0.1:5173/settings",
        "title": "Settings",
        "readyState": "complete",
        "readiness": {"fonts": "loaded", "pendingImages": 0, "totalImages": 0},
        "viewport": {"width": 1000, "height": 800, "dpr": 1.0},
        "scroll": {"x": 0, "y": 0},
        "activeRef": "@save",
        "semantic_tree": {
            "ref": "@root",
            "tag": "main",
            "role": "main",
            "name": null,
            "description": null,
            "rect": {"x": 0.0, "y": 0.0, "width": 1000.0, "height": 800.0},
            "documentRect": {"x": 0.0, "y": 0.0, "width": 1000.0, "height": 800.0},
            "interactive": false,
            "states": {},
            "visibility": {"inViewport": true},
            "sourceHint": null,
            "attributes": {},
            "style": null,
            "children": [{
                "ref": "@card",
                "tag": "section",
                "role": "region",
                "name": "Settings card",
                "description": null,
                "rect": {"x": 200.0, "y": 160.0, "width": 600.0, "height": 420.0},
                "documentRect": {"x": 200.0, "y": 160.0, "width": 600.0, "height": 420.0},
                "interactive": false,
                "states": {},
                "visibility": {"inViewport": true},
                "sourceHint": {
                    "origin": "data-component-source",
                    "file": "SettingsCard.tsx",
                    "line": 10,
                    "column": 2
                },
                "attributes": {},
                "style": null,
                "children": [{
                    "ref": "@save",
                    "tag": "button",
                    "role": "button",
                    "name": "Save",
                    "description": null,
                    "rect": {"x": 320.0, "y": 300.0, "width": 100.0, "height": 40.0},
                    "documentRect": {"x": 320.0, "y": 300.0, "width": 100.0, "height": 40.0},
                    "interactive": true,
                    "states": {},
                    "visibility": {"inViewport": true},
                    "sourceHint": {
                        "origin": "data-component-source",
                        "file": "SettingsCard.tsx",
                        "line": 35,
                        "column": 5
                    },
                    "attributes": {"type": "button"},
                    "style": null,
                    "children": []
                }]
            }]
        },
        "interactive": [],
        "occlusion": {"max_samples": 128, "sampled": 0},
        "delta": {"added_refs": [], "removed_refs": [], "changed_refs": [], "layout_changes": [], "route_changed": false}
    })
}

async fn complete_next_snapshot(state: ControlState, session_id: uuid::Uuid, payload: Value) {
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
                .expect("fresh snapshot action must be inflight before completion");
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: claimed.id,
                        ok: true,
                        error: None,
                        payload,
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("fresh semantic endpoint did not enqueue a snapshot action");
}

async fn get_fresh(
    state: ControlState,
    session_id: uuid::Uuid,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/semantic-snapshot/fresh"));
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(builder.body(Body::empty()).expect("fresh snapshot request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded fresh snapshot body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn get_fresh_with_result(
    state: ControlState,
    session_id: uuid::Uuid,
    payload: Value,
) -> (StatusCode, Value) {
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(executor_state, session_id, payload).await;
    });
    let response = get_fresh(state, session_id, true).await;
    executor.await.expect("snapshot executor task");
    response
}

#[tokio::test]
async fn fresh_semantic_snapshot_requires_auth_and_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        get_fresh(state.clone(), session_id, false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get_fresh(state, uuid::Uuid::new_v4(), true).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn fresh_semantic_snapshot_projects_the_matching_new_action_result() {
    let (state, session_id) = test_state().await;
    let (status, body) = get_fresh_with_result(state, session_id, raw_snapshot_payload()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], 9);
    assert_eq!(body["route"], "http://127.0.0.1:5173/settings");
    assert_eq!(body["viewport"], serde_json::json!([1000, 800]));
    assert_eq!(body["root"]["reference"], "@root");
    assert_eq!(body["root"]["children"][0]["reference"], "@card");
    assert_eq!(body["root"]["children"][0]["source"]["file"], "SettingsCard.tsx");
    assert_eq!(body["root"]["children"][0]["source"]["component"], "SettingsCard.tsx");
    assert_eq!(body["root"]["children"][0]["children"][0]["reference"], "@save");
    assert!(body["captured_at"].is_string());
    assert_eq!(body["console_errors"], serde_json::json!([]));
    assert_eq!(body["failed_requests"], serde_json::json!([]));
}

#[tokio::test]
async fn malformed_matching_snapshot_result_fails_closed() {
    let (state, session_id) = test_state().await;
    let (status, body) = get_fresh_with_result(
        state,
        session_id,
        serde_json::json!({"version": 1, "semantic_tree": null}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(body["error"], "invalid_fresh_semantic_snapshot");
}

#[tokio::test]
async fn stale_or_unrelated_results_cannot_satisfy_a_new_fresh_snapshot_request() {
    let (state, session_id) = test_state().await;
    let unrelated = state
        .live
        .enqueue_action(session_id, None, BridgeActionKind::Snapshot)
        .await;
    let claimed = state
        .live
        .claim_action(session_id, unrelated.id)
        .await
        .expect("unrelated action must be claimable");
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

    let (status, body) = get_fresh(state, session_id, true).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["error"], "fresh_semantic_snapshot_timeout");
}
