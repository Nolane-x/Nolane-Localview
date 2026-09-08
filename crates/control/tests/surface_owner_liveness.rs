#![recursion_limit = "256"]

use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_sessions::SessionManager;
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize)]
struct Registration {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

fn test_state() -> ControlState {
    ControlState {
        token: Arc::from("test-token"),
        sessions: Arc::new(SessionManager::new(Duration::from_secs(2))),
        observations: ObservationBus::new(32),
        live: LiveBridge::new(64, 8),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    }
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

async fn register_owner(state: ControlState, owner_instance_id: Uuid) -> Registration {
    let (status, value) = send(
        state,
        "/v1/runtime/resources/surfaces/owners/register",
        serde_json::json!({ "owner_instance_id": owner_instance_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_value(value).expect("registration body")
}

fn proof(registration: Registration) -> Value {
    serde_json::json!({
        "owner_instance_id": registration.owner_instance_id,
        "boot_epoch": registration.boot_epoch,
        "owner_lease_id": registration.owner_lease_id,
    })
}

#[test]
fn owner_liveness_policy_is_bounded_and_has_a_deterministic_reaper_seam() {
    let owner = include_str!("../src/surface_owner.rs");
    let runtime = include_str!("../src/resource_runtime.rs");

    assert!(
        owner.contains("SURFACE_OWNER_TTL") && owner.contains("Duration::from_secs(15)"),
        "owner authority needs the approved conservative 15-second TTL"
    );
    assert!(
        owner.contains("reap_expired_surface_owners_for_sessions_at"),
        "owner expiry needs an explicit deterministic-time reaper seam"
    );
    assert!(
        runtime.contains("/v1/runtime/resources/surfaces/owners/heartbeat"),
        "the control plane must expose the exact owner heartbeat route"
    );
}

#[tokio::test]
async fn heartbeat_requires_the_exact_current_boot_owner_proof() {
    let state = test_state();
    let registration = register_owner(state.clone(), Uuid::new_v4()).await;

    let (status, _) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/owners/heartbeat",
        proof(registration),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "current proof must refresh liveness");

    let stale = Registration {
        owner_lease_id: Uuid::new_v4(),
        ..registration
    };
    let (status, body) = send(
        state,
        "/v1/runtime/resources/surfaces/owners/heartbeat",
        proof(stale),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_lease_mismatch");
}
