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
use localview_evidence::EvidenceStore;
use localview_live_bridge::{
    LiveBridge, NetworkFaultControlCommand, NetworkFaultControlResult, NetworkFaultLeaseAuthority,
};
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
            cwd: Some(format!("/tmp/localview-wave3-network-fault-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Wave3 network fault {port}")),
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

async fn execute_one_private_control(live: LiveBridge, session_id: Uuid) {
    for _ in 0..150 {
        let controls = live.take_network_fault_controls(session_id, 8).await;
        if let Some(control) = controls.into_iter().next() {
            let request_id = control.id;
            let claimed = live
                .claim_network_fault_control(session_id, request_id)
                .await
                .expect("exact private control claim");
            let payload = match claimed.command {
                NetworkFaultControlCommand::Install { plan, .. } => {
                    let fingerprint = plan["fingerprint"]
                        .as_str()
                        .expect("canonical fingerprint")
                        .to_owned();
                    let rule_count = plan["rules"].as_array().map(Vec::len).unwrap_or(0);
                    let remaining_ms = plan["lease_ms"].as_u64().unwrap_or(0);
                    serde_json::json!({
                        "installed": true,
                        "active": true,
                        "fingerprint": fingerprint,
                        "rule_count": rule_count,
                        "total_hits": 0,
                        "remaining_ms": remaining_ms,
                        "surface_incarnation": 1,
                    })
                }
                NetworkFaultControlCommand::Clear { .. } => serde_json::json!({
                    "cleared": true,
                    "active": false,
                    "surface_incarnation": 1,
                }),
            };
            assert!(
                live.complete_network_fault_control(
                    session_id,
                    NetworkFaultControlResult {
                        request_id,
                        ok: true,
                        error: None,
                        payload,
                        completed_at: Utc::now(),
                    },
                )
                .await
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("private network fault control was not delivered");
}

async fn execute_install_with_mismatched_ack(live: LiveBridge, session_id: Uuid) -> Uuid {
    for _ in 0..150 {
        let controls = live.take_network_fault_controls(session_id, 8).await;
        if let Some(control) = controls.into_iter().next() {
            let request_id = control.id;
            let claimed = live
                .claim_network_fault_control(session_id, request_id)
                .await
                .expect("exact private control claim");
            let lease_token = match claimed.command {
                NetworkFaultControlCommand::Install { lease_token, .. } => lease_token,
                NetworkFaultControlCommand::Clear { .. } => {
                    panic!("expected install before cleanup")
                }
            };
            assert!(
                live.complete_network_fault_control(
                    session_id,
                    NetworkFaultControlResult {
                        request_id,
                        ok: true,
                        error: None,
                        payload: serde_json::json!({
                            "installed": true,
                            "active": true,
                            "fingerprint": "ffffffffffffffff",
                            "rule_count": 2,
                            "remaining_ms": 1000,
                            "surface_incarnation": 1,
                        }),
                        completed_at: Utc::now(),
                    },
                )
                .await
            );
            return lease_token;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("private network fault install was not delivered");
}

fn plan() -> Value {
    serde_json::json!({
        "rules": [
            {
                "id": "fail-data",
                "transport": "fetch",
                "method": "get",
                "path": "/api/data",
                "effect": {"kind": "fail"},
                "max_hits": 1
            },
            {
                "id": "delay-save",
                "transport": "both",
                "method": "post",
                "path": "/api/save",
                "effect": {"kind": "delay", "milliseconds": 120},
                "max_hits": 2
            }
        ],
        "lease_ms": 1500
    })
}

#[tokio::test]
async fn install_status_and_clear_require_exact_private_ack() {
    let (state, owner, other) = test_state().await;

    let (unauthorized, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults"),
        Some(plan()),
        false,
    )
    .await;
    assert_eq!(unauthorized, StatusCode::UNAUTHORIZED);

    let worker = tokio::spawn(execute_one_private_control(state.live.clone(), owner));
    let (created, body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults"),
        Some(plan()),
        true,
    )
    .await;
    worker.await.expect("install executor");
    assert_eq!(created, StatusCode::CREATED, "{body}");
    assert_eq!(body["active"], true);
    assert_eq!(body["rule_count"], 2);
    assert_eq!(
        body["fingerprint"].as_str().map(str::len),
        Some(16),
        "{body}"
    );
    let lease_id = body["lease_id"].as_str().expect("public lease id").to_owned();
    let encoded = body.to_string();
    assert!(!encoded.contains("lease_token"));
    assert!(!encoded.contains("fail-data"));
    assert!(!encoded.contains("/api/data"));

    assert!(
        state.live.take_public_actions(owner, 8).await.is_empty(),
        "private network control must not leak into public page actions"
    );

    let (status, current) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/network-faults"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{current}");
    assert_eq!(current["active"], true);
    assert_eq!(current["lease_id"], lease_id);

    let (stale_invalidation, stale_body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults/invalidate-preview"),
        Some(serde_json::json!({"surface_incarnation": 2})),
        true,
    )
    .await;
    assert_eq!(stale_invalidation, StatusCode::NO_CONTENT, "{stale_body}");
    let (still_active_status, still_active) = send(
        state.clone(),
        Method::GET,
        format!("/v1/sessions/{owner}/network-faults"),
        None,
        true,
    )
    .await;
    assert_eq!(still_active_status, StatusCode::OK);
    assert_eq!(still_active["active"], true, "stale incarnation must not clear current lease");

    let (cross_status, cross_body) = send(
        state.clone(),
        Method::DELETE,
        format!("/v1/sessions/{other}/network-faults/{lease_id}"),
        None,
        true,
    )
    .await;
    assert_eq!(cross_status, StatusCode::NOT_FOUND, "{cross_body}");

    let clear_worker = tokio::spawn(execute_one_private_control(state.live.clone(), owner));
    let (cleared, clear_body) = send(
        state.clone(),
        Method::DELETE,
        format!("/v1/sessions/{owner}/network-faults/{lease_id}"),
        None,
        true,
    )
    .await;
    clear_worker.await.expect("clear executor");
    assert_eq!(cleared, StatusCode::NO_CONTENT, "{clear_body}");

    let (status, inactive) = send(
        state,
        Method::GET,
        format!("/v1/sessions/{owner}/network-faults"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inactive}");
    assert_eq!(inactive["active"], false);
}


#[tokio::test]
async fn mismatched_install_ack_fails_closed_and_queues_exact_cleanup() {
    let (state, owner, _) = test_state().await;
    let previous_lease_id = Uuid::new_v4();
    let previous_token = Uuid::new_v4();
    state
        .live
        .set_network_fault_lease(
            owner,
            NetworkFaultLeaseAuthority {
                lease_id: previous_lease_id,
                lease_token: previous_token,
                fingerprint: "aaaaaaaaaaaaaaaa".into(),
                rule_count: 1,
                surface_incarnation: 1,
                expires_at: Utc::now() + chrono::Duration::seconds(30),
            },
        )
        .await;

    let worker = tokio::spawn(execute_install_with_mismatched_ack(
        state.live.clone(),
        owner,
    ));
    let (status, body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults"),
        Some(plan()),
        true,
    )
    .await;
    let new_token = worker.await.expect("mismatched install executor");

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["error"], "network_fault_preview_ack_mismatch");
    assert!(
        state.live.network_fault_lease(owner).await.is_none(),
        "daemon must not retain the superseded previous lease after a committed-but-invalid install ack"
    );

    let cleanup = state.live.take_network_fault_controls(owner, 8).await;
    assert_eq!(cleanup.len(), 1, "committed mismatch needs one exact cleanup");
    assert!(matches!(
        cleanup[0].command,
        NetworkFaultControlCommand::Clear { lease_token } if lease_token == new_token
    ));
    assert_ne!(new_token, previous_token);
}

#[tokio::test]
async fn schema_policy_rejects_unknown_or_noncanonical_authority() {
    let (state, owner, _) = test_state().await;
    let mut unknown = plan();
    unknown
        .as_object_mut()
        .expect("plan object")
        .insert("arbitrary_width".into(), Value::from(123));

    let (unknown_status, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults"),
        Some(unknown),
        true,
    )
    .await;
    assert_eq!(unknown_status, StatusCode::UNPROCESSABLE_ENTITY);

    let invalid = serde_json::json!({
        "rules": [{
            "id": "external",
            "transport": "fetch",
            "method": "get",
            "path": "https://example.com/api",
            "effect": {"kind": "fail"},
            "max_hits": 1
        }],
        "lease_ms": 1000
    });
    let (invalid_status, invalid_body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{owner}/network-faults"),
        Some(invalid),
        true,
    )
    .await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid_body}");
    assert_eq!(invalid_body["error"], "network_fault_invalid_plan");
}
