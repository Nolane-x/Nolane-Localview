use std::{
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
            cwd: Some("/tmp/localview-css-trace-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("CSS Trace Test".into()),
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
        "version": 12,
        "route": "http://127.0.0.1:5173/settings",
        "viewport": {"width": 1000, "height": 800, "dpr": 1.0},
        "semantic_tree": {
            "ref": "@root",
            "tag": "main",
            "role": "main",
            "name": null,
            "rect": {"x": 0.0, "y": 0.0, "width": 1000.0, "height": 800.0},
            "interactive": false,
            "attributes": {},
            "sourceHint": null,
            "style": null,
            "styleTrace": null,
            "children": [{
                "ref": "@save",
                "tag": "button",
                "role": "button",
                "name": "Save",
                "rect": {"x": 320.0, "y": 300.0, "width": 100.0, "height": 40.0},
                "interactive": true,
                "attributes": {"type": "button"},
                "sourceHint": null,
                "style": {
                    "display": "flex",
                    "paddingTop": "16px",
                    "color": "rgb(20, 20, 20)"
                },
                "styleTrace": {
                    "declarations": [
                        {
                            "source_kind": "inline_element",
                            "stylesheet_path": null,
                            "selector": null,
                            "property": "display",
                            "value": "flex",
                            "important": false
                        },
                        {
                            "source_kind": "same_origin_stylesheet",
                            "stylesheet_path": "src/button.css",
                            "selector": ".save",
                            "property": "padding-top",
                            "value": "16px",
                            "important": false
                        }
                    ],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": true,
                        "unresolved_properties": ["color"],
                        "winners": [{
                            "source_kind": "inline_element",
                            "stylesheet_path": null,
                            "selector": null,
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [1, 0, 0, 0],
                            "source_order": 0
                        }]
                    }
                },
                "children": []
            }]
        }
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
                .expect("style trace Snapshot action must become inflight");
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
    panic!("style trace endpoint did not enqueue a Snapshot action");
}

async fn get_trace(
    state: ControlState,
    session_id: uuid::Uuid,
    reference: &str,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/style-trace/{reference}"));
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(builder.body(Body::empty()).expect("style trace request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded style trace body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn fresh_style_trace_requires_authentication() {
    let (state, session_id) = test_state().await;
    let (status, body) = get_trace(state, session_id, "@save", false).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn fresh_style_trace_returns_only_selected_bounded_css_evidence() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(executor_state, session_id, raw_snapshot_payload()).await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["reference"], "@save");
    assert_eq!(body["computed"]["display"], "flex");
    assert_eq!(body["computed"]["paddingTop"], "16px");
    assert_eq!(body["declarations"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["declarations"][1]["stylesheet_path"], "src/button.css");
    assert_eq!(body["declarations"][1]["selector"], ".save");
    assert_eq!(
        body["author_cascade"]["scope"],
        "supported_author_subset"
    );
    assert_eq!(body["author_cascade"]["coverage_complete"], true);
    assert_eq!(
        body["author_cascade"]["unresolved_properties"][0],
        "color"
    );
    assert_eq!(
        body["author_cascade"]["winners"][0]["property"],
        "display"
    );
    assert_eq!(
        body["author_cascade"]["winners"][0]["specificity"],
        serde_json::json!([1, 0, 0, 0])
    );
    assert!(body.get("semantic_tree").is_none());
}
