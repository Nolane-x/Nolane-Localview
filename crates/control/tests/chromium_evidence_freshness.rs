#![recursion_limit = "256"]

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
use localview_evidence::{
    EvidenceDraft, EvidenceKind, EvidenceStore, Provenance, UncertaintyClass,
};
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use tower::ServiceExt;

#[tokio::test]
async fn same_revision_chromium_evidence_does_not_suppress_a_new_route() {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(
            vec![DiscoveredServer {
                candidate: ListenerCandidate {
                    endpoint: Endpoint {
                        host: "127.0.0.1".into(),
                        port: 5173,
                        scheme: "http".into(),
                    },
                    pid: Some(42),
                    process_name: Some("node".into()),
                    command: Some("vite".into()),
                    cwd: None,
                },
                classification: Classification {
                    kind: ServerKind::FrontendDevServer,
                    confidence: 1.0,
                    framework: Some("Vite".into()),
                    title: None,
                    hmr_detected: true,
                    evidence: Default::default(),
                },
            }],
            Utc::now(),
        )
        .await;
    let session_id = reconcile.created[0];
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::new(64, 8),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };

    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![
                ObserverEvent {
                    seq: 1,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::SemanticSnapshot,
                    reference: None,
                    route: Some("http://127.0.0.1:5173/new-route".into()),
                    payload: serde_json::json!({"version": 2}),
                },
                ObserverEvent {
                    seq: 2,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Layout,
                    reference: None,
                    route: Some("http://127.0.0.1:5173/new-route".into()),
                    payload: serde_json::json!({"verified": true}),
                },
            ],
        })
        .await;

    state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Contract,
            session_id,
            region: None,
            payload: serde_json::json!({
                "probe": "page_load_dump_dom",
                "target": "http://127.0.0.1:5173/old-route",
                "exit_code": 0
            }),
            provenance: Provenance {
                source: "chromium-compatibility".into(),
                engine: Some("chromium".into()),
                revision: Some("same-revision".into()),
                parent_ids: Vec::new(),
                captured_at: Utc::now(),
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        })
        .await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/perception/plan"))
        .header(header::AUTHORIZATION, "Bearer test-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "budget": {
                    "latency_ms": 5_000,
                    "text_tokens": 800,
                    "image_regions": 0,
                    "chromium_spawns": 1
                },
                "deep_mode": false,
                "compatibility_requested": true,
                "target": "@save",
                "revision": "same-revision"
            })
            .to_string(),
        ))
        .expect("request");
    let response = router(state).oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["signals"]["browser_specific_suspicion"], true);
    assert_eq!(
        json["plan"]["actions"][0]["action"]["kind"],
        "chromium_escalation"
    );
}
