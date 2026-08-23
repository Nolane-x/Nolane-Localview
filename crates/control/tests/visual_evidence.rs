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

#[tokio::test]
async fn visual_evidence_metadata_is_ingested_without_pixel_payload() {
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

    let payload = serde_json::json!({
        "artifact_id": "lv-123",
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
    });
    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/evidence/visual"))
        .header(header::AUTHORIZATION, "Bearer test-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .expect("valid visual evidence request");

    let response = router(state)
        .oneshot(request)
        .await
        .expect("control router response");
    assert_eq!(response.status(), StatusCode::OK);

    let recent = evidence.recent_for_session(session_id, 10).await;
    assert_eq!(recent.len(), 1);
    let visual = &recent[0];
    assert_eq!(visual.kind, EvidenceKind::Visual);
    assert_eq!(visual.provenance.source, "native-capture");
    assert_eq!(visual.payload["artifact_id"], "lv-123");
    let stored = visual.payload.to_string();
    assert!(!stored.contains("png"));
    assert!(!stored.contains("base64"));
}
