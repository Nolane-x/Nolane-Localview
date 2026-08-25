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
use localview_evidence::{
    EvidenceDraft, EvidenceKind, EvidenceStore, Provenance, UncertaintyClass,
};
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
            cwd: Some("/tmp/localview-perception-feedback-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Perception Feedback Test".into()),
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

fn step_body() -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": 1500,
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
    method: Method,
    uri: String,
    body: Value,
) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(method)
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
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json control response")
    };
    (status, value)
}

async fn post_step(state: ControlState, session_id: uuid::Uuid) -> (StatusCode, Value) {
    post_json(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/step"),
        step_body(),
    )
    .await
}

async fn post_plan(state: ControlState, session_id: uuid::Uuid) -> (StatusCode, Value) {
    post_json(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/plan"),
        step_body(),
    )
    .await
}

async fn complete_next_snapshot_through_control(state: ControlState, session_id: uuid::Uuid) {
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
                payload: raw_snapshot_payload(),
                completed_at: Utc::now(),
            };
            let (status, _) = post_json(
                state,
                Method::POST,
                format!("/v1/sessions/{session_id}/actions/results"),
                serde_json::to_value(result).expect("snapshot result json"),
            )
            .await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("perception step did not enqueue a semantic snapshot");
}

async fn insert_untrusted_snapshot_evidence(state: &ControlState, session_id: uuid::Uuid) {
    for kind in [EvidenceKind::Semantic, EvidenceKind::Layout] {
        state
            .evidence
            .insert(EvidenceDraft {
                kind,
                session_id,
                region: None,
                payload: raw_snapshot_payload(),
                provenance: Provenance {
                    source: "untrusted-test-source".into(),
                    engine: Some("native-webview".into()),
                    revision: None,
                    parent_ids: Vec::new(),
                    captured_at: Utc::now(),
                },
                confidence: 1.0,
                uncertainty: UncertaintyClass::Observed,
                secret_taint: false,
            })
            .await;
    }
}

#[tokio::test]
async fn executed_semantic_snapshot_becomes_retained_planner_evidence_for_the_next_step() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot_through_control(executor_state, session_id).await;
    });

    let (first_status, first_body) = post_step(state.clone(), session_id).await;
    executor.await.expect("semantic executor task");
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(
        first_body["plan"]["actions"][0]["action"]["kind"],
        "semantic_snapshot"
    );

    let retained = state.evidence.recent_for_session(session_id, 32).await;
    assert!(retained.iter().any(|evidence| {
        evidence.kind == EvidenceKind::Semantic
            && evidence.provenance.source == "native-semantic-snapshot"
    }));
    assert!(retained.iter().any(|evidence| {
        evidence.kind == EvidenceKind::Layout
            && evidence.provenance.source == "native-semantic-snapshot"
    }));

    let (second_status, second_body) = post_step(state.clone(), session_id).await;
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(
        second_body["plan"]["actions"].as_array().map(Vec::len),
        Some(0),
        "the next planner cycle must consume retained semantic/layout evidence instead of repeating the snapshot"
    );
    assert!(second_body["engine"].is_null());
    assert!(second_body["execution"].is_null());
    assert!(state.live.take_actions(session_id, 8).await.is_empty());
}

#[tokio::test]
async fn arbitrary_retained_semantic_and_layout_evidence_cannot_suppress_required_observation() {
    let (state, session_id) = test_state().await;
    insert_untrusted_snapshot_evidence(&state, session_id).await;

    let (status, body) = post_plan(state, session_id).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["plan"]["actions"][0]["action"]["kind"],
        "semantic_snapshot",
        "retained evidence without native snapshot provenance must not become perception authority"
    );
}
