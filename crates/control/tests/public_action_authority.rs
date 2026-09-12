use std::{
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Body,
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
use serde_json::{json, Value};
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
            pid: Some(77),
            process_name: Some("vite".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/public-action-authority".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: None,
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn fixture() -> (axum::Router, LiveBridge, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    let live = LiveBridge::new(32, 8);
    let app = router(ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(16),
        live: live.clone(),
        evidence: EvidenceStore::default(),
        paused: Arc::new(AtomicBool::new(false)),
    });
    (app, live, session_id)
}

async fn post_action(app: &axum::Router, session_id: Uuid, action: Value) -> StatusCode {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/actions"))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "reference": null,
                        "action": action,
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn legacy_public_route_rejects_all_consequential_action_kinds_without_enqueueing() {
    let (app, live, session_id) = fixture().await;
    let consequential = [
        json!({"type": "click"}),
        json!({"type": "type_text", "text": "hello", "clear_first": true}),
        json!({"type": "key", "key": "Enter", "modifiers": []}),
        json!({"type": "scroll", "x": 0.0, "y": 120.0}),
        json!({"type": "focus"}),
    ];

    for action in consequential {
        assert_eq!(
            post_action(&app, session_id, action).await,
            StatusCode::CONFLICT,
            "legacy public consequential actions must require the canonical V4.3 authority path"
        );
    }

    assert!(
        live.take_actions(session_id, 64).await.is_empty(),
        "rejected consequential actions must not mutate the legacy action queue"
    );
}

#[tokio::test]
async fn legacy_public_route_keeps_observe_only_snapshot_available() {
    let (app, live, session_id) = fixture().await;

    assert_eq!(
        post_action(&app, session_id, json!({"type": "snapshot"})).await,
        StatusCode::ACCEPTED
    );

    let queued = live.take_actions(session_id, 64).await;
    assert_eq!(queued.len(), 1);
    assert!(matches!(
        queued[0].action,
        localview_live_bridge::BridgeActionKind::Snapshot
    ));
}
