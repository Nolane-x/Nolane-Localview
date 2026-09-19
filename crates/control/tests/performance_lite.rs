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
use localview_evidence::EvidenceStore;
use localview_live_bridge::{
    LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind,
};
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
            pid: Some(42),
            process_name: Some("node".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/localview-performance-lite-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Performance Lite Test".into()),
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

async fn get(state: ControlState, session_id: Uuid, authorized: bool) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/performance-lite"));
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }

    let response = router(state)
        .oneshot(
            builder
                .body(Body::empty())
                .expect("performance-lite request"),
        )
        .await
        .expect("control router response");
    let status = response.status();
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded performance-lite response");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

fn performance_event(seq: u64, duration: f64, marker: &str) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at: Utc::now(),
        kind: ObserverEventKind::Performance,
        reference: None,
        route: Some(format!("/private?token={marker}")),
        payload: json!({
            "type": "long_task",
            "duration": duration,
            "raw": marker
        }),
    }
}

#[tokio::test]
async fn performance_lite_requires_auth_and_known_session() {
    let (state, session_id) = test_state().await;

    assert_eq!(
        get(state.clone(), session_id, false).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(state, Uuid::new_v4(), true).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn performance_lite_is_exact_session_bounded_and_payload_free() {
    let (state, session_id) = test_state().await;

    let events = (0_u64..20)
        .map(|offset| performance_event(offset + 1, 50.0 + offset as f64, "MUST-NOT-LEAK"))
        .chain([
            ObserverEvent {
                seq: 21,
                captured_at: Utc::now(),
                kind: ObserverEventKind::Performance,
                reference: None,
                route: Some("/private-layout".into()),
                payload: json!({"type":"layout_shift","value":0.1,"raw":"LAYOUT-SECRET"}),
            },
            ObserverEvent {
                seq: 22,
                captured_at: Utc::now(),
                kind: ObserverEventKind::Performance,
                reference: None,
                route: Some("/private-layout".into()),
                payload: json!({"type":"layout_shift","value":0.2,"raw":"LAYOUT-SECRET"}),
            },
        ])
        .collect::<Vec<_>>();

    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events,
        })
        .await;

    let foreign_session = Uuid::new_v4();
    state
        .live
        .ingest(ObserverBatch {
            session_id: foreign_session,
            generation: 1,
            events: vec![performance_event(1, 999.0, "FOREIGN-SECRET")],
        })
        .await;

    let (status, value) = get(state, session_id, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["version"], 1);
    assert_eq!(value["long_task_count"], 20);
    assert_eq!(value["total_long_task_ms"], 1_190);
    assert_eq!(value["max_long_task_ms"], 69);
    assert_eq!(value["applied_long_task_sample_budget"], 8);
    assert_eq!(value["omitted_long_task_samples"], 12);

    let samples = value["sampled_long_tasks_ms"]
        .as_array()
        .expect("bounded long-task sample array");
    assert_eq!(samples.len(), 8);
    assert_eq!(samples.first().and_then(Value::as_u64), Some(69));
    assert_eq!(samples.last().and_then(Value::as_u64), Some(62));

    let cls = value["cumulative_layout_shift"]
        .as_f64()
        .expect("cumulative layout shift");
    assert!((cls - 0.3).abs() < 1e-9);

    let serialized = serde_json::to_string(&value).expect("response serialization");
    for forbidden in [
        "MUST-NOT-LEAK",
        "LAYOUT-SECRET",
        "FOREIGN-SECRET",
        "/private",
        "token=",
        "999",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "performance-lite packet leaked forbidden value: {forbidden}"
        );
    }
}
