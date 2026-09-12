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
use localview_control::{
    configure_surface_recovery_journal_for_sessions,
    release_surface_resource_session_for_sessions, router,
    runtime_resource_governor_for_sessions, ControlState, SurfaceRecoveryJournal,
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
            cwd: Some("/tmp/localview-hidden-surface-cleanup-baseline".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Hidden Surface Cleanup Baseline".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid, OwnerRegistration) {
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
        "localview-hidden-surface-cleanup-recovery-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = Arc::new(
        SurfaceRecoveryJournal::open(journal_path)
            .await
            .expect("open test surface recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&state.sessions, Some(journal));
    let owner = register_owner(state.clone()).await;
    (state, session_id, owner)
}

async fn register_owner(state: ControlState) -> OwnerRegistration {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/runtime/resources/surfaces/owners/register")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::from(
                    serde_json::json!({ "owner_instance_id": Uuid::new_v4() }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let owner: OwnerRegistration = serde_json::from_slice(&bytes).expect("owner registration");
    assert!(!owner.recovery_required);
    owner
}

async fn send(state: ControlState, uri: &str, body: Value) -> StatusCode {
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
    let _ = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    status
}

fn reserve_body(session_id: Uuid, request_id: &str, owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn activate_body(
    session_id: Uuid,
    request_id: &str,
    incarnation: u64,
    owner: OwnerRegistration,
) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "surface_kind": "preview_window",
        "label": "preview-cleanup-baseline",
        "incarnation": incarnation,
        "visibility": "hidden",
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn visibility_body(
    session_id: Uuid,
    incarnation: u64,
    visibility: &str,
    owner: OwnerRegistration,
) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-cleanup-baseline",
        "incarnation": incarnation,
        "visibility": visibility,
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

fn release_body(session_id: Uuid, incarnation: u64, owner: OwnerRegistration) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-cleanup-baseline",
        "incarnation": incarnation,
        "owner_instance_id": owner.owner_instance_id,
        "boot_epoch": owner.boot_epoch,
        "owner_lease_id": owner.owner_lease_id
    })
}

#[tokio::test]
async fn repeated_exact_surface_lifecycles_return_control_and_governor_to_baseline() {
    let (state, session_id, owner) = test_state().await;
    let governor = runtime_resource_governor_for_sessions(&state.sessions);

    for incarnation in 1..=32_u64 {
        let request_id = format!("open-{incarnation}");
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/reserve",
                reserve_body(session_id, &request_id, owner),
            )
            .await,
            StatusCode::NO_CONTENT,
            "cycle {incarnation}: prior owner state must not consume the next surface admission"
        );
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/activate",
                activate_body(session_id, &request_id, incarnation, owner),
            )
            .await,
            StatusCode::NO_CONTENT,
            "cycle {incarnation}: exact pending reservation must activate"
        );
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/visibility",
                visibility_body(session_id, incarnation, "visible", owner),
            )
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/visibility",
                visibility_body(session_id, incarnation, "hidden", owner),
            )
            .await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(
                state.clone(),
                "/v1/runtime/resources/surfaces/release",
                release_body(session_id, incarnation, owner),
            )
            .await,
            StatusCode::NO_CONTENT,
            "cycle {incarnation}: exact physical-owner release must clear live authority"
        );

        assert_eq!(
            release_surface_resource_session_for_sessions(&state.sessions, session_id),
            0,
            "cycle {incarnation}: control surface authority must have no pending or live residue"
        );
        assert_eq!(
            governor.release_session(&session_id.to_string()),
            0,
            "cycle {incarnation}: governor must already be back at reservation baseline"
        );
    }
}
