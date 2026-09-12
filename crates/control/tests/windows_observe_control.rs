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
use localview_control::{
    configure_windows_observe_runtime_for_sessions, router, ControlState,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
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
            cwd: Some("/tmp/windows-observe-control".into()),
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

async fn fixture() -> (axum::Router, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    configure_windows_observe_runtime_for_sessions(&sessions, None);
    let app = router(ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(16),
        live: LiveBridge::new(32, 8),
        evidence: EvidenceStore::default(),
        paused: Arc::new(AtomicBool::new(false)),
    });
    (app, session_id)
}

#[tokio::test]
async fn windows_observe_routes_require_control_bearer_auth() {
    let (app, session_id) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/sessions/{session_id}/windows-observe/status"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn existing_session_fails_closed_when_windows_runtime_is_unavailable() {
    let (app, session_id) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{session_id}/windows-observe/attach"))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "native_window_handle": 4660,
                        "expected_process_id": 77,
                        "selection_nonce": Uuid::from_u128(0x4402),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn unknown_session_is_rejected_before_runtime_lookup() {
    let (app, _) = fixture().await;
    let missing = Uuid::from_u128(0x4403);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/sessions/{missing}/windows-observe/attach"))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "native_window_handle": 4660,
                        "expected_process_id": 77,
                        "selection_nonce": Uuid::from_u128(0x4402),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
