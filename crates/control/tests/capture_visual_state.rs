use std::{
    sync::{
        atomic::AtomicBool,
        Arc,
    },
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
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
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
            cwd: Some("/tmp/localview-visual-state-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Visual State Test".into()),
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

async fn post(
    state: ControlState,
    uri: String,
    authorized: bool,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method("POST").uri(uri);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(serde_json::to_vec(&value).expect("request JSON"))
        }
        None => Body::empty(),
    };
    let response = router(state)
        .oneshot(builder.body(body).expect("visual state request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded response body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn complete_next_freeze(state: ControlState, session_id: Uuid) -> Uuid {
    for _ in 0..100 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions
            .into_iter()
            .find(|action| matches!(&action.action, BridgeActionKind::FreezeVisuals))
        {
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("freeze action must be inflight before completion");
            let action_id = claimed.id;
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id,
                        ok: true,
                        error: None,
                        payload: serde_json::json!({
                            "paused_animations": 7,
                            "web_animations_supported": true,
                            "private_page_payload": "must-not-escape"
                        }),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return action_id;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("capture freeze did not enqueue FreezeVisuals");
}

async fn complete_next_restore(state: ControlState, session_id: Uuid, expected_token: Uuid) {
    for _ in 0..100 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions.into_iter().find(|action| {
            matches!(
                &action.action,
                BridgeActionKind::RestoreVisuals { token } if *token == expected_token
            )
        }) {
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("restore action must be inflight before completion");
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: claimed.id,
                        ok: true,
                        error: None,
                        payload: serde_json::json!({"private_page_payload": "must-not-escape"}),
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("capture restore did not enqueue matching RestoreVisuals");
}

#[tokio::test]
async fn capture_visual_state_requires_auth_and_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(
        post(
            state.clone(),
            format!("/v1/sessions/{session_id}/capture-freeze"),
            false,
            None,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post(
            state,
            format!("/v1/sessions/{}/capture-freeze", Uuid::new_v4()),
            true,
            None,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn capture_freeze_returns_only_bounded_acknowledged_metadata() {
    let (state, session_id) = test_state().await;
    let executor = tokio::spawn(complete_next_freeze(state.clone(), session_id));
    let (status, body) = post(
        state,
        format!("/v1/sessions/{session_id}/capture-freeze"),
        true,
        None,
    )
    .await;
    let action_id = executor.await.expect("freeze executor task");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["token"], action_id.to_string());
    assert_eq!(body["paused_animations"], 7);
    assert_eq!(body["web_animations_supported"], true);
    assert_eq!(body["lease_ms"], 8_000);
    let encoded = body.to_string();
    assert!(!encoded.contains("private_page_payload"));
    assert!(!encoded.contains("must-not-escape"));
}

#[tokio::test]
async fn capture_restore_queues_matching_token_and_requires_exact_acknowledgement() {
    let (state, session_id) = test_state().await;
    let token = Uuid::new_v4();
    let executor = tokio::spawn(complete_next_restore(state.clone(), session_id, token));
    let (status, body) = post(
        state,
        format!("/v1/sessions/{session_id}/capture-restore"),
        true,
        Some(serde_json::json!({"token": token})),
    )
    .await;
    executor.await.expect("restore executor task");

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
}

#[tokio::test]
async fn generic_action_queue_rejects_internal_visual_state_actions() {
    let (state, session_id) = test_state().await;
    for action in [
        serde_json::json!({"type": "freeze_visuals"}),
        serde_json::json!({"type": "restore_visuals", "token": Uuid::new_v4()}),
    ] {
        let (status, body) = post(
            state.clone(),
            format!("/v1/sessions/{session_id}/actions"),
            true,
            Some(serde_json::json!({"reference": null, "action": action})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "internal_capture_action_not_public");
    }
}
