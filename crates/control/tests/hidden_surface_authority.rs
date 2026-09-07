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
    release_surface_resource_session_for_sessions, router,
    runtime_resource_governor_for_sessions, ControlState,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::Value;
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
            cwd: Some("/tmp/localview-hidden-surface-authority".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Hidden Surface Authority".into()),
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

async fn send(
    state: ControlState,
    method: Method,
    uri: &str,
    body: Value,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(builder.body(Body::from(body.to_string())).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn reserve_body(session_id: Uuid, request_id: &str) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id
    })
}

fn activate_body(session_id: Uuid, request_id: &str, incarnation: u64) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "surface_kind": "preview_window",
        "label": "preview-contract",
        "incarnation": incarnation,
        "visibility": "hidden"
    })
}

fn visibility_body(session_id: Uuid, incarnation: u64, visibility: &str) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-contract",
        "incarnation": incarnation,
        "visibility": visibility
    })
}

fn release_body(session_id: Uuid, incarnation: u64) -> Value {
    serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-contract",
        "incarnation": incarnation
    })
}

#[tokio::test]
async fn exact_surface_lifecycle_is_authenticated_and_incarnation_safe() {
    let (state, session_id) = test_state().await;

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "open-1"),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "surface owner lifecycle routes must require the control token"
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "open-1"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "open-1"),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "duplicate exact pending surface request must not double-admit"
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "open-1", 1),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/visibility",
            visibility_body(session_id, 0, "visible"),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "stale incarnation may not mutate current surface visibility"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/visibility",
            visibility_body(session_id, 1, "visible"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 0),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "stale close may not release a newer surface incarnation"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 1),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state,
            Method::POST,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 1),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "an already released surface incarnation has no remaining owner authority"
    );
}

#[tokio::test]
async fn activation_requires_the_exact_pending_request() {
    let (state, session_id) = test_state().await;

    assert_eq!(
        send(
            state,
            Method::POST,
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "missing", 1),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "platform creation cannot forge a live lease without prior resource admission"
    );
}

#[tokio::test]
async fn exact_pending_surface_reservation_can_be_cancelled_without_touching_live_owner() {
    let (state, session_id) = test_state().await;

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "live-owner"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "live-owner", 9),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "create-failed"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/cancel",
            reserve_body(session_id, "create-failed"),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED,
        "pending reservation cancellation must require the control token"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/cancel",
            reserve_body(session_id, "create-failed"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "failed platform creation must be able to cancel its exact pending reservation"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "create-failed"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "exact cancellation must drop the reservation and free the request id for retry"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/cancel",
            reserve_body(session_id, "create-failed"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/cancel",
            reserve_body(session_id, "live-owner"),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "cancellation may not forge release of an already activated live owner"
    );
    assert_eq!(
        send(
            state,
            Method::POST,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 9),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "live owner authority must remain intact after stale pending cancellation"
    );
}

#[tokio::test]
async fn session_cleanup_releases_pending_but_not_live_surface_owner_truth() {
    let (state, session_id) = test_state().await;

    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "pending"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        release_surface_resource_session_for_sessions(&state.sessions, session_id),
        1,
        "surface control cleanup must release pending owner work"
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "live"),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            Method::POST,
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "live", 3),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let governor = runtime_resource_governor_for_sessions(&state.sessions);
    assert_eq!(
        governor.release_session(&session_id.to_string()),
        0,
        "generic governor session cleanup must retain live surface leases"
    );
    assert_eq!(
        release_surface_resource_session_for_sessions(&state.sessions, session_id),
        0,
        "surface control cleanup must not forge live desktop owner exit"
    );
    assert_eq!(
        send(
            state,
            Method::POST,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 3),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "exact live owner release must still succeed after generic session cleanup"
    );
}
