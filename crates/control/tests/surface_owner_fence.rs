#![recursion_limit = "256"]

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
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct Registration {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
    recovery_required: bool,
}

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
            cwd: Some("/tmp/localview-d2-surface-owner".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("D2 Surface Owner Fence".into()),
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
        live: LiveBridge::new(64, 8),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

async fn send(state: ControlState, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn register_owner(state: ControlState, owner_instance_id: Uuid) -> (StatusCode, Registration) {
    let (status, value) = send(
        state,
        "/v1/runtime/resources/surfaces/owners/register",
        serde_json::json!({ "owner_instance_id": owner_instance_id }),
    )
    .await;
    let registration = serde_json::from_value(value).expect("registration body");
    (status, registration)
}

fn reserve_body(
    session_id: Uuid,
    request_id: &str,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "owner_instance_id": owner_instance_id,
        "boot_epoch": boot_epoch,
        "owner_lease_id": owner_lease_id,
    })
}

#[tokio::test]
async fn current_boot_registration_fences_surface_reservations() {
    let (state, session_id) = test_state().await;
    let owner = Uuid::new_v4();

    let (status, registration) = register_owner(state.clone(), owner).await;
    assert_eq!(status, StatusCode::OK, "owner registration must be explicit");
    assert_eq!(registration.owner_instance_id, owner);
    assert!(!registration.boot_epoch.is_nil());
    assert!(!registration.owner_lease_id.is_nil());
    assert!(!registration.recovery_required);

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(
            session_id,
            "wrong-epoch",
            owner,
            Uuid::new_v4(),
            registration.owner_lease_id,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_boot_epoch_mismatch");

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(
            session_id,
            "wrong-lease",
            owner,
            registration.boot_epoch,
            Uuid::new_v4(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_lease_mismatch");

    let (status, _) = send(
        state,
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(
            session_id,
            "valid-owner",
            owner,
            registration.boot_epoch,
            registration.owner_lease_id,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
