#![recursion_limit = "256"]

use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{
    configure_surface_recovery_journal_for_sessions, router, ControlState, SurfaceRecoveryJournal,
    SurfaceRecoveryKey,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize)]
struct OwnerRegistration {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
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
            cwd: Some("/tmp/localview-surface-recovery-activation".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Surface Recovery Activation".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn state() -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    (
        ControlState {
            token: Arc::from("test-token"),
            sessions,
            observations: ObservationBus::new(32),
            live: LiveBridge::new(64, 8),
            evidence: EvidenceStore::new(128),
            paused: Arc::new(AtomicBool::new(false)),
        },
        session_id,
    )
}

fn temp_journal_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-surface-recovery-activation-{}.jsonl",
        Uuid::new_v4()
    ))
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

async fn register(state: ControlState, owner_instance_id: Uuid) -> OwnerRegistration {
    let (status, value) = send(
        state,
        "/v1/runtime/resources/surfaces/owners/register",
        serde_json::json!({ "owner_instance_id": owner_instance_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_value(value).expect("owner registration")
}

fn reserve_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": "open-recovery",
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn activate_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": "open-recovery",
        "surface_kind": "preview_window",
        "label": "preview-recovery",
        "incarnation": 1,
        "visibility": "hidden",
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn release_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-recovery",
        "incarnation": 1,
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn recovery_key(session_id: Uuid, owner: OwnerRegistration) -> SurfaceRecoveryKey {
    SurfaceRecoveryKey::new(
        session_id,
        "preview_window",
        "preview-recovery",
        1,
        owner.owner_instance_id,
    )
    .expect("valid recovery key")
}

#[tokio::test]
async fn activation_is_published_only_after_exact_recovery_debt_is_durable() {
    let (state, session_id) = state().await;
    let path = temp_journal_path();
    let journal = Arc::new(
        SurfaceRecoveryJournal::open(&path)
            .await
            .expect("open recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&state.sessions, Some(journal.clone()));
    let owner = register(state.clone(), Uuid::new_v4()).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let key = recovery_key(session_id, owner);
    assert!(
        journal.outstanding_exact(&key),
        "activation success must imply crash-replayable exact recovery debt"
    );

    assert_eq!(
        send(
            state,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert!(
        !journal.outstanding_exact(&key),
        "exact clean release must durably discharge recovery debt"
    );
    let _ = tokio::fs::remove_file(path).await;
}

#[tokio::test]
async fn activation_fails_closed_when_recovery_journal_is_unavailable() {
    let (state, session_id) = state().await;
    let owner = register(state.clone(), Uuid::new_v4()).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, body) = send(
        state,
        "/v1/runtime/resources/surfaces/activate",
        activate_body(session_id, owner),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body.get("error").and_then(Value::as_str),
        Some("surface_recovery_journal_unavailable")
    );
}
