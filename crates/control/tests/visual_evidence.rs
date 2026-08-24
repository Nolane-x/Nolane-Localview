use std::{
    sync::{
        atomic::AtomicBool,
        Arc,
    },
    time::Duration,
};

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore};
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
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
            cwd: Some("/tmp/localview-visual-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Visual Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, uuid::Uuid, EvidenceStore) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    let evidence = EvidenceStore::new(128);
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: evidence.clone(),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id, evidence)
}

fn valid_payload() -> serde_json::Value {
    serde_json::json!({
        "artifact_id": "lv-0123456789abcdef",
        "pixel_width": 1280,
        "pixel_height": 820,
        "backend": "webview2",
        "route": "http://127.0.0.1:5173/",
        "viewport": {
            "css_width": 1280,
            "css_height": 820,
            "device_scale_factor": 1.0
        },
        "revision": "abc123",
        "captured_at_unix_ms": 123,
        "target": "viewport"
    })
}

async fn post_visual(
    state: ControlState,
    session_id: uuid::Uuid,
    payload: serde_json::Value,
) -> StatusCode {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/evidence/visual"))
        .header(header::AUTHORIZATION, "Bearer test-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .expect("valid visual evidence request");

    router(state)
        .oneshot(request)
        .await
        .expect("control router response")
        .status()
}

#[tokio::test]
async fn visual_evidence_metadata_is_ingested_without_pixel_payload() {
    let (state, session_id, evidence) = test_state().await;
    let status = post_visual(state, session_id, valid_payload()).await;
    assert_eq!(status, StatusCode::OK);

    let recent = evidence.recent_for_session(session_id, 10).await;
    assert_eq!(recent.len(), 1);
    let visual = &recent[0];
    assert_eq!(visual.kind, EvidenceKind::Visual);
    assert_eq!(visual.provenance.source, "native-capture");
    assert_eq!(visual.payload["artifact_id"], "lv-0123456789abcdef");
    let stored = visual.payload.to_string();
    assert!(!stored.contains("png"));
    assert!(!stored.contains("base64"));
}

#[tokio::test]
async fn visual_evidence_rejects_non_loopback_route() {
    let (state, session_id, evidence) = test_state().await;
    let mut payload = valid_payload();
    payload["route"] = serde_json::json!("https://example.com/");
    assert_eq!(
        post_visual(state, session_id, payload).await,
        StatusCode::BAD_REQUEST
    );
    assert!(evidence.recent_for_session(session_id, 10).await.is_empty());
}

#[tokio::test]
async fn visual_evidence_rejects_unknown_backend() {
    let (state, session_id, evidence) = test_state().await;
    let mut payload = valid_payload();
    payload["backend"] = serde_json::json!("untrusted-capture-engine");
    assert_eq!(
        post_visual(state, session_id, payload).await,
        StatusCode::BAD_REQUEST
    );
    assert!(evidence.recent_for_session(session_id, 10).await.is_empty());
}

#[tokio::test]
async fn visual_evidence_rejects_non_content_artifact_id_and_negative_time() {
    let (state, session_id, evidence) = test_state().await;
    let mut bad_id = valid_payload();
    bad_id["artifact_id"] = serde_json::json!("arbitrary-id");
    assert_eq!(
        post_visual(state.clone(), session_id, bad_id).await,
        StatusCode::BAD_REQUEST
    );

    let mut bad_time = valid_payload();
    bad_time["captured_at_unix_ms"] = serde_json::json!(-1);
    assert_eq!(
        post_visual(state, session_id, bad_time).await,
        StatusCode::BAD_REQUEST
    );
    assert!(evidence.recent_for_session(session_id, 10).await.is_empty());
}
