use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use chrono::{Duration as ChronoDuration, Utc};
use localview_control::{router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore, UncertaintyClass};
use localview_live_bridge::{BridgeActionKind, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn discovered(port: u16) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: "http".into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("node".into()),
            command: Some("vite".into()),
            cwd: Some(format!("/tmp/localview-wave3-correlation-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Wave3 correlation {port}")),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered(5173), discovered(5174)], Utc::now())
        .await;
    assert_eq!(reconcile.created.len(), 2);
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: EvidenceStore::new(256),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, reconcile.created[0], reconcile.created[1])
}

async fn send(
    state: ControlState,
    method: Method,
    uri: String,
    body: Option<Value>,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state)
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn correlation_requires_auth_known_session_and_completed_action() {
    let (state, owner, other) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, None, BridgeActionKind::Snapshot)
        .await;

    let (unauthorized, _) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions/{}/correlation", action.id),
        None,
        false,
    )
    .await;
    assert_eq!(unauthorized, StatusCode::UNAUTHORIZED);

    let (cross_session, body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{other}/actions/{}/correlation", action.id),
        None,
        true,
    )
    .await;
    assert_eq!(cross_session, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "completed_action_boundary_not_found");

    let (not_completed, body) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/actions/{}/correlation", action.id),
        None,
        true,
    )
    .await;
    assert_eq!(not_completed, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "completed_action_boundary_not_found");
}

#[tokio::test]
async fn correlation_uses_daemon_boundary_and_retains_exact_parent_evidence() {
    let (state, owner, _) = test_state().await;
    let action = state
        .live
        .enqueue_action(owner, Some("@e1".into()), BridgeActionKind::Snapshot)
        .await;

    let (take_status, take_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions"),
        None,
        true,
    )
    .await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body.as_array().map(Vec::len), Some(1));

    let observed = Utc::now();
    let observer_body = serde_json::json!({
        "session_id": owner,
        "generation": 1,
        "events": [
            {
                "seq": 1,
                "captured_at": observed,
                "kind": "network",
                "reference": null,
                "route": "http://127.0.0.1:5173/",
                "payload": {"method": "POST", "status": 200}
            },
            {
                "seq": 2,
                "captured_at": observed + ChronoDuration::milliseconds(5),
                "kind": "dom_mutation",
                "reference": "@e1",
                "route": "http://127.0.0.1:5173/",
                "payload": {"changed_refs": ["@e1"]}
            }
        ]
    });
    let (observer_status, observer_response) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/observer"),
        Some(observer_body),
        true,
    )
    .await;
    assert_eq!(observer_status, StatusCode::OK, "{observer_response}");

    let (result_status, result_body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/actions/results"),
        Some(serde_json::json!({
            "action_id": action.id,
            "ok": true,
            "error": null,
            "payload": {"route": "http://127.0.0.1:5173/"},
            "completed_at": Utc::now()
        })),
        true,
    )
    .await;
    assert_eq!(result_status, StatusCode::NO_CONTENT, "{result_body}");

    let boundary = state
        .live
        .action_execution_boundary(owner, action.id)
        .await
        .expect("daemon-owned boundary");
    assert!(boundary.started_at <= boundary.completed_at);

    let (early_status, early_body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions/{}/correlation", action.id),
        None,
        true,
    )
    .await;
    assert_eq!(early_status, StatusCode::CONFLICT, "{early_body}");
    assert_eq!(early_body["error"], "correlation_window_open");

    tokio::time::sleep(Duration::from_millis(1_550)).await;

    let (status, body) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/actions/{}/correlation", action.id),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["trace"]["action_id"], action.id.to_string());
    assert_eq!(body["trace"]["links"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["trace"]["links"][0]["basis"], "temporal_window");
    assert!(
        body["trace"]["links"][0]["confidence"]
            .as_f64()
            .is_some_and(|value| value <= 0.551),
        "temporal association confidence must stay at the conservative ~0.55 cap: {body}"
    );
    assert_eq!(body["window"]["basis"], "daemon_execution_boundary");

    let causal_id = body["evidence_id"]
        .as_str()
        .expect("causal evidence id")
        .to_owned();
    let causal = state.evidence.get(&causal_id).await.expect("causal evidence");
    assert_eq!(causal.kind, EvidenceKind::Causal);
    assert_eq!(causal.uncertainty, UncertaintyClass::Derived);
    assert!(!causal.secret_taint);
    assert!(causal.provenance.parent_ids.len() >= 3);

    let mut parents = Vec::new();
    for parent_id in &causal.provenance.parent_ids {
        if let Some(parent) = state.evidence.get(parent_id).await {
            parents.push(parent);
        }
    }
    assert!(parents.iter().any(|item| item.kind == EvidenceKind::Interaction));
    assert!(parents.iter().any(|item| item.kind == EvidenceKind::Network));
    assert!(parents.iter().any(|item| item.kind == EvidenceKind::Semantic));

    let (repeat_status, repeat_body) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/actions/{}/correlation", action.id),
        None,
        true,
    )
    .await;
    assert_eq!(repeat_status, StatusCode::OK, "{repeat_body}");
    assert_eq!(repeat_body["evidence_id"], causal_id);
    assert_eq!(repeat_body["deduplicated"], true);
}
