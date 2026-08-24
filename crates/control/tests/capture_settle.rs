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
use chrono::{Duration as ChronoDuration, Utc};
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
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
            cwd: Some("/tmp/localview-settle-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Settle Test".into()),
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

fn semantic(seq: u64, captured_at: chrono::DateTime<Utc>) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at,
        kind: ObserverEventKind::SemanticSnapshot,
        reference: None,
        route: Some("http://127.0.0.1:5173/".into()),
        payload: serde_json::json!({
            "type": "semantic_snapshot",
            "snapshot": {
                "readyState": "complete",
                "readiness": {
                    "fonts": "loaded",
                    "pendingImages": 0,
                    "totalImages": 1
                },
                "privatePayloadThatMustNotReturn": "secret"
            }
        }),
    }
}

fn event(seq: u64, kind: ObserverEventKind, captured_at: chrono::DateTime<Utc>) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at,
        kind,
        reference: None,
        route: Some("http://127.0.0.1:5173/".into()),
        payload: serde_json::json!({"private": "must-not-return"}),
    }
}

async fn ingest(state: &ControlState, session_id: uuid::Uuid, events: Vec<ObserverEvent>) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events,
        })
        .await;
}

async fn get_settle(
    state: ControlState,
    session_id: uuid::Uuid,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/capture-settle"));
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(builder.body(Body::empty()).expect("settle request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded settle body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn capture_settle_requires_auth_and_known_session() {
    let (state, session_id) = test_state().await;
    assert_eq!(get_settle(state.clone(), session_id, false).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        get_settle(state, uuid::Uuid::new_v4(), true).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn capture_settle_reports_missing_semantic_snapshot() {
    let (state, session_id) = test_state().await;
    let (status, body) = get_settle(state, session_id, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stable"], false);
    assert!(body["reasons"]
        .as_array()
        .expect("settle reasons")
        .contains(&Value::String("no_semantic_snapshot".into())));
}

#[tokio::test]
async fn capture_settle_accepts_ready_and_quiet_page_without_leaking_snapshot_payload() {
    let (state, session_id) = test_state().await;
    ingest(
        &state,
        session_id,
        vec![semantic(1, Utc::now() - ChronoDuration::seconds(2))],
    )
    .await;

    let (status, body) = get_settle(state, session_id, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["stable"], true);
    let encoded = body.to_string();
    assert!(!encoded.contains("privatePayloadThatMustNotReturn"));
    assert!(!encoded.contains("secret"));
}

#[tokio::test]
async fn capture_settle_reports_recent_hmr_layout_and_network_independently() {
    let (state, session_id) = test_state().await;
    let old = Utc::now() - ChronoDuration::seconds(2);
    let now = Utc::now();
    ingest(
        &state,
        session_id,
        vec![
            semantic(1, old),
            event(2, ObserverEventKind::Hmr, now),
            event(3, ObserverEventKind::Layout, now),
            event(4, ObserverEventKind::Network, now),
        ],
    )
    .await;

    let (status, body) = get_settle(state, session_id, true).await;
    assert_eq!(status, StatusCode::OK);
    let reasons = body["reasons"].as_array().expect("settle reasons");
    for reason in ["hmr_recent", "layout_recent", "network_recent"] {
        assert!(reasons.contains(&Value::String(reason.into())), "missing {reason}");
    }
}
