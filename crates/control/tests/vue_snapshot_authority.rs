use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{ControlState, router};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
use serde_json::Value;
use tokio::fs;
use tower::ServiceExt;
use uuid::Uuid;

fn discovered(cwd: &Path) -> DiscoveredServer {
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
            cwd: Some(cwd.to_string_lossy().into_owned()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vue".into()),
            title: Some("Vue Snapshot Authority Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state(cwd: &Path) -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered(cwd)], Utc::now()).await;
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

fn snapshot_payload(file: String) -> Value {
    serde_json::json!({
        "version": 1,
        "route": "http://127.0.0.1:5173/",
        "viewport": {"width": 800, "height": 600},
        "semantic_tree": {
            "ref": "@root",
            "tag": "main",
            "role": "main",
            "name": null,
            "rect": {"x": 0.0, "y": 0.0, "width": 800.0, "height": 600.0},
            "interactive": false,
            "attributes": {},
            "sourceHint": null,
            "children": [{
                "ref": "@vue",
                "tag": "button",
                "role": "button",
                "name": "Save",
                "rect": {"x": 20.0, "y": 20.0, "width": 120.0, "height": 40.0},
                "interactive": true,
                "attributes": {},
                "sourceHint": {
                    "origin": "vue-dev-instance",
                    "file": file,
                    "component": "VueCard",
                    "signal": "element_parent_component"
                },
                "children": []
            }]
        }
    })
}

async fn complete_next_snapshot_via_control(state: ControlState, session_id: Uuid, payload: Value) {
    for _ in 0..120 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions
            .into_iter()
            .find(|action| matches!(&action.action, BridgeActionKind::Snapshot))
        {
            let result = BridgeActionResult {
                action_id: action.id,
                ok: true,
                error: None,
                payload,
                completed_at: Utc::now(),
            };
            let response = router(state)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/v1/sessions/{session_id}/actions/results"))
                        .header(header::AUTHORIZATION, "Bearer test-token")
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            serde_json::to_vec(&result).expect("serialize result"),
                        ))
                        .expect("snapshot completion request"),
                )
                .await
                .expect("snapshot completion response");
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("fresh semantic endpoint did not enqueue a snapshot action");
}

fn json_contains_string_fragment(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value.contains(needle),
        Value::Array(values) => values
            .iter()
            .any(|value| json_contains_string_fragment(value, needle)),
        Value::Object(values) => values
            .values()
            .any(|value| json_contains_string_fragment(value, needle)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

async fn get_json(state: ControlState, uri: String) -> (StatusCode, Vec<u8>, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .expect("GET request"),
        )
        .await
        .expect("GET response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response")
        .to_vec();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, bytes, value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn absolute_vue_path_is_canonicalized_before_fresh_results_and_evidence_are_retained() {
    let root =
        std::env::temp_dir().join(format!("localview-vue-http-authority-{}", Uuid::new_v4()));
    let source_dir = root.join("src");
    fs::create_dir_all(&source_dir).await.unwrap();
    let source = source_dir.join("VueCard.vue");
    fs::write(&source, b"<template><button>Save</button></template>")
        .await
        .unwrap();

    let absolute = fs::canonicalize(&source)
        .await
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (state, session_id) = test_state(&root).await;

    let executor_state = state.clone();
    let executor_payload = snapshot_payload(absolute.clone());
    let executor = tokio::spawn(async move {
        complete_next_snapshot_via_control(executor_state, session_id, executor_payload).await;
    });

    let (fresh_status, _fresh_bytes, fresh) = get_json(
        state.clone(),
        format!("/v1/sessions/{session_id}/semantic-snapshot/fresh"),
    )
    .await;
    executor.await.expect("snapshot executor");

    assert_eq!(fresh_status, StatusCode::OK);
    assert_eq!(fresh["root"]["children"][0]["source"], Value::Null);
    assert_eq!(
        fresh["root"]["children"][0]["ownership"]["file"],
        "src/VueCard.vue"
    );
    assert_eq!(
        fresh["root"]["children"][0]["ownership"]["component"],
        "VueCard"
    );
    assert!(!json_contains_string_fragment(&fresh, &absolute));

    let (results_status, _results_bytes, results) = get_json(
        state.clone(),
        format!("/v1/sessions/{session_id}/actions/results"),
    )
    .await;
    assert_eq!(results_status, StatusCode::OK);
    assert!(!json_contains_string_fragment(&results, &absolute));

    let (evidence_status, _evidence_bytes, evidence) =
        get_json(state, format!("/v1/sessions/{session_id}/evidence/recent")).await;
    assert_eq!(evidence_status, StatusCode::OK);
    assert!(!json_contains_string_fragment(&evidence, &absolute));
    assert!(json_contains_string_fragment(&evidence, "src/VueCard.vue"));

    let _ = fs::remove_dir_all(root).await;
}
