#![recursion_limit = "256"]

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{
    ControlState, SURFACE_RECOVERY_JOURNAL_FILE, SurfaceRecoveryJournal, SurfaceRecoveryKey,
    configure_surface_recovery_journal_for_sessions, router,
    runtime_resource_governor_for_sessions,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::{SESSION_IDENTITY_REGISTRY_FILE, SessionIdentityResolver, SessionManager};
use serde::Deserialize;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize)]
struct OwnerRegistration {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
    recovery_required: bool,
}

fn temp_state_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "localview-surface-recovery-reattach-{}",
        Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("create durable test state root");
    root
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

fn discovered(port: u16) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: if port == 5173 { "http" } else { "https" }.into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("node".into()),
            command: Some(if port == 5173 { "vite" } else { "vite --host" }.into()),
            cwd: Some("/work/d2-restart-surface".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("D2 Reattach".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

fn control_state(sessions: Arc<SessionManager>) -> ControlState {
    ControlState {
        token: Arc::from("test-token"),
        sessions,
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

fn owner_fields(owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn reserve_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "request_id": "pre-crash-open"
    });
    body.as_object_mut().expect("reserve body").extend(
        owner_fields(owner)
            .as_object()
            .expect("owner fields")
            .clone(),
    );
    body
}

fn activate_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "request_id": "pre-crash-open",
        "surface_kind": "preview_window",
        "label": "preview-survived-daemon",
        "incarnation": 1,
        "visibility": "hidden"
    });
    body.as_object_mut().expect("activate body").extend(
        owner_fields(owner)
            .as_object()
            .expect("owner fields")
            .clone(),
    );
    body
}

fn reattach_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-survived-daemon",
        "incarnation": 1,
        "visibility": "hidden"
    });
    body.as_object_mut().expect("reattach body").extend(
        owner_fields(owner)
            .as_object()
            .expect("owner fields")
            .clone(),
    );
    body
}

fn release_body(session_id: Uuid, owner: OwnerRegistration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-survived-daemon",
        "incarnation": 1
    });
    body.as_object_mut().expect("release body").extend(
        owner_fields(owner)
            .as_object()
            .expect("owner fields")
            .clone(),
    );
    body
}

#[tokio::test]
async fn exact_debt_reattaches_same_durable_session_with_fresh_boot_authority() {
    let root = temp_state_root();
    let identity_path = root.join(SESSION_IDENTITY_REGISTRY_FILE);
    let recovery_path = root.join(SURFACE_RECOVERY_JOURNAL_FILE);
    let owner_instance_id = Uuid::new_v4();

    let first_sessions = Arc::new(SessionManager::with_identity_resolver(
        Duration::from_secs(2),
        SessionIdentityResolver::open_file(identity_path.clone()).await,
    ));
    let first_reconcile = first_sessions
        .reconcile(vec![discovered(5173)], Utc::now())
        .await;
    let durable_session_id = first_reconcile.created[0];
    let first_state = control_state(first_sessions.clone());
    let first_journal = Arc::new(
        SurfaceRecoveryJournal::open(&recovery_path)
            .await
            .expect("open first-boot recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&first_sessions, Some(first_journal.clone()));
    let first_owner = register(first_state.clone(), owner_instance_id).await;
    assert!(!first_owner.recovery_required);

    assert_eq!(
        send(
            first_state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(durable_session_id, first_owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            first_state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(durable_session_id, first_owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let debt_key = SurfaceRecoveryKey::new(
        durable_session_id,
        "preview_window",
        "preview-survived-daemon",
        1,
        owner_instance_id,
    )
    .expect("exact debt key");
    assert!(first_journal.outstanding_exact(&debt_key));

    drop(first_state);
    drop(first_sessions);
    drop(first_journal);

    let second_sessions = Arc::new(SessionManager::with_identity_resolver(
        Duration::from_secs(2),
        SessionIdentityResolver::open_file(identity_path).await,
    ));
    let second_reconcile = second_sessions
        .reconcile(vec![discovered(8443)], Utc::now())
        .await;
    assert_eq!(
        second_reconcile.created,
        vec![durable_session_id],
        "D1 durable identity must survive the daemon restart"
    );
    let second_state = control_state(second_sessions.clone());
    let second_journal = Arc::new(
        SurfaceRecoveryJournal::open(&recovery_path)
            .await
            .expect("replay second-boot recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&second_sessions, Some(second_journal.clone()));
    assert!(second_journal.outstanding_exact(&debt_key));

    let second_owner = register(second_state.clone(), owner_instance_id).await;
    assert!(second_owner.recovery_required);
    assert_ne!(second_owner.boot_epoch, first_owner.boot_epoch);
    assert_ne!(second_owner.owner_lease_id, first_owner.owner_lease_id);

    let replacement_owner = register(second_state.clone(), Uuid::new_v4()).await;
    assert!(!replacement_owner.recovery_required);
    let (status, body) = send(
        second_state.clone(),
        "/v1/runtime/resources/surfaces/reattach",
        reattach_body(durable_session_id, replacement_owner),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_recovery_debt_missing");

    let (status, body) = send(
        second_state.clone(),
        "/v1/runtime/resources/surfaces/reattach",
        reattach_body(durable_session_id, first_owner),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_boot_epoch_mismatch");

    let governor = runtime_resource_governor_for_sessions(&second_sessions);
    assert_eq!(governor.release_session(&durable_session_id.to_string()), 0);
    assert_eq!(
        send(
            second_state.clone(),
            "/v1/runtime/resources/surfaces/reattach",
            reattach_body(durable_session_id, second_owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "exact surviving owner debt must create fresh current-boot live authority"
    );
    assert!(
        !second_journal.outstanding_exact(&debt_key),
        "durable Reattached must discharge exact boot recovery debt"
    );

    let (status, body) = send(
        second_state.clone(),
        "/v1/runtime/resources/surfaces/reattach",
        reattach_body(durable_session_id, second_owner),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_recovery_debt_missing");

    assert_eq!(
        send(
            second_state,
            "/v1/runtime/resources/surfaces/release",
            release_body(durable_session_id, second_owner),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        governor.release_session(&durable_session_id.to_string()),
        0,
        "exact reattach/release must return fresh governor accounting to baseline"
    );
    cleanup(&root);
}
