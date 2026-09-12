#![recursion_limit = "256"]

use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{
    ControlState, SurfaceRecoveryJournal, configure_surface_recovery_journal_for_sessions,
    reap_expired_surface_owner_resources_for_sessions_at, router,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
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
            cwd: Some("/tmp/localview-surface-owner-liveness".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Surface Owner Liveness".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn surface_state() -> (ControlState, Uuid, Registration) {
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
    let journal_path = std::env::temp_dir().join(format!(
        "localview-surface-owner-liveness-recovery-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = Arc::new(
        SurfaceRecoveryJournal::open(journal_path)
            .await
            .expect("open liveness recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&state.sessions, Some(journal));
    let registration = register_owner(state.clone(), Uuid::new_v4()).await;
    (state, session_id, registration)
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

fn reserve_body(session_id: Uuid, request_id: &str, owner: Registration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id,
    })
}

fn activate_body(session_id: Uuid, request_id: &str, owner: Registration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "surface_kind": "preview_window",
        "label": "liveness-preview",
        "incarnation": 1,
        "visibility": "hidden",
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id,
    })
}

fn reattach_body(session_id: Uuid, owner: Registration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "liveness-preview",
        "incarnation": 1,
        "visibility": "hidden",
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id,
    })
}

#[test]
fn owner_liveness_policy_is_bounded_and_has_a_deterministic_reaper_seam() {
    let owner = include_str!("../src/surface_owner.rs");
    let control = include_str!("../src/lib.rs");

    assert!(
        owner.contains("SURFACE_OWNER_TTL") && owner.contains("Duration::from_secs(15)"),
        "owner authority needs the approved conservative 15-second TTL"
    );
    assert!(
        owner.contains("reap_expired_surface_owners_for_sessions_at"),
        "owner expiry needs an explicit deterministic-time reaper seam"
    );
    assert!(
        control.contains("surface_liveness") && control.contains("surface_liveness::router"),
        "owner heartbeat must be integrated as a dedicated control-plane liveness router"
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
    assert_eq!(
        status,
        StatusCode::NO_CONTENT,
        "current proof must refresh liveness"
    );

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

#[tokio::test]
async fn expired_owner_reaper_drops_pending_and_live_governor_state_without_transferring_debt() {
    let (state, session_id, owner_a) = surface_state().await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "live-a", owner_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "live-a", owner_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "pending-a", owner_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let removed = reap_expired_surface_owner_resources_for_sessions_at(
        &state.sessions,
        Instant::now() + Duration::from_secs(16),
    );
    assert_eq!(
        removed, 2,
        "expiry must drop both the pending reservation and live surface lease"
    );

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/owners/heartbeat",
        proof(owner_a),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_not_registered");

    let owner_b = register_owner(state.clone(), Uuid::new_v4()).await;
    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/reattach",
        reattach_body(session_id, owner_b),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["error"], "surface_recovery_debt_missing",
        "a replacement owner must never adopt the expired owner's recovery debt"
    );

    for index in 0..4 {
        let request_id = format!("baseline-{index}");
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/reserve",
                reserve_body(session_id, &request_id, owner_b),
            )
            .await
            .0,
            StatusCode::NO_CONTENT,
            "expired owner resources must return NativeSurface governor accounting to baseline"
        );
    }
}

#[test]
fn daemon_runs_surface_owner_reaper_on_five_second_interval() {
    let daemon = include_str!("../../../apps/daemon/src/main.rs");

    assert!(
        daemon.contains("SURFACE_OWNER_REAP_INTERVAL") && daemon.contains("Duration::from_secs(5)"),
        "daemon must run the owner reaper on a bounded five-second interval"
    );
    assert!(
        daemon.contains("reap_expired_surface_owner_resources_for_sessions"),
        "daemon must actively revoke expired surface owner resources even when no new request arrives"
    );
}
