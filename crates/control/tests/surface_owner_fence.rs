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
    configure_surface_recovery_journal_for_sessions, router, ControlState, SurfaceRecoveryJournal,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{BridgeActionKind, LiveBridge, ProviderObservationBinding};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, EventContinuityState, ListenerCandidate, ProviderIncarnationRef, ServerKind, TargetIncarnationRef};
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
    let journal_path = std::env::temp_dir().join(format!(
        "localview-surface-owner-fence-recovery-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = Arc::new(
        SurfaceRecoveryJournal::open(journal_path)
            .await
            .expect("open test surface recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&state.sessions, Some(journal));
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

fn owner_fields(registration: Registration) -> Value {
    serde_json::json!({
        "owner_instance_id": registration.owner_instance_id,
        "boot_epoch": registration.boot_epoch,
        "owner_lease_id": registration.owner_lease_id,
    })
}

fn reserve_body(session_id: Uuid, request_id: &str, registration: Registration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
    });
    body.as_object_mut()
        .expect("reserve body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn activate_body(
    session_id: Uuid,
    request_id: &str,
    incarnation: u64,
    registration: Registration,
) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "surface_kind": "preview_window",
        "label": "preview-owner-fence",
        "incarnation": incarnation,
        "visibility": "hidden",
    });
    body.as_object_mut()
        .expect("activate body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn visibility_body(
    session_id: Uuid,
    incarnation: u64,
    visibility: &str,
    registration: Registration,
) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-owner-fence",
        "incarnation": incarnation,
        "visibility": visibility,
    });
    body.as_object_mut()
        .expect("visibility body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn release_body(session_id: Uuid, incarnation: u64, registration: Registration) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "preview-owner-fence",
        "incarnation": incarnation,
    });
    body.as_object_mut()
        .expect("release body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn activate_workspace_body(
    session_id: Uuid,
    request_id: &str,
    incarnation: u64,
    registration: Registration,
) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "request_id": request_id,
        "surface_kind": "workspace_child",
        "label": "workspace-owner-fence",
        "incarnation": incarnation,
        "visibility": "visible",
    });
    body.as_object_mut()
        .expect("workspace activate body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn action_surface_body(
    session_id: Uuid,
    surface_kind: &str,
    label: &str,
    incarnation: u64,
    registration: Registration,
) -> Value {
    let mut body = serde_json::json!({
        "session_id": session_id,
        "surface_kind": surface_kind,
        "label": label,
        "incarnation": incarnation,
    });
    body.as_object_mut()
        .expect("surface action body object")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    body
}

fn complete_action_body(
    session_id: Uuid,
    action_id: Uuid,
    registration: Registration,
) -> Value {
    let mut body = action_surface_body(
        session_id,
        "preview_window",
        "preview-owner-fence",
        1,
        registration,
    );
    body.as_object_mut().expect("complete action body object").insert(
        "result".into(),
        serde_json::json!({
            "action_id": action_id,
            "ok": true,
            "error": null,
            "payload": {"confirmed": true},
            "completed_at": Utc::now(),
        }),
    );
    body
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

    let wrong_epoch = Registration {
        boot_epoch: Uuid::new_v4(),
        ..registration
    };
    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(session_id, "wrong-epoch", wrong_epoch),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_boot_epoch_mismatch");

    let wrong_lease = Registration {
        owner_lease_id: Uuid::new_v4(),
        ..registration
    };
    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(session_id, "wrong-lease", wrong_lease),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_lease_mismatch");

    let (status, _) = send(
        state,
        "/v1/runtime/resources/surfaces/reserve",
        reserve_body(session_id, "valid-owner", registration),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn live_surface_mutations_are_fenced_by_exact_owner_instance_and_current_lease() {
    let (state, session_id) = test_state().await;
    let owner_a = Uuid::new_v4();
    let owner_b = Uuid::new_v4();
    let (_, registration_a) = register_owner(state.clone(), owner_a).await;
    let (_, registration_b) = register_owner(state.clone(), owner_b).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "owner-a-open", registration_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "owner-a-open", 1, registration_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "activation must consume the exact owner's pending reservation"
    );

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/visibility",
        visibility_body(session_id, 1, "visible", registration_b),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_fence_mismatch");

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/release",
        release_body(session_id, 1, registration_b),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_fence_mismatch");

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/visibility",
            visibility_body(session_id, 1, "visible", registration_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let (_, rotated_a) = register_owner(state.clone(), owner_a).await;
    assert_eq!(rotated_a.owner_instance_id, owner_a);
    assert_eq!(rotated_a.boot_epoch, registration_a.boot_epoch);
    assert_ne!(rotated_a.owner_lease_id, registration_a.owner_lease_id);

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/visibility",
        visibility_body(session_id, 1, "hidden", registration_a),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "surface_owner_lease_mismatch");

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/visibility",
            visibility_body(session_id, 1, "hidden", rotated_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT,
        "lease rotation must preserve the same process owner identity while revoking the old capability"
    );
    assert_eq!(
        send(
            state,
            "/v1/runtime/resources/surfaces/release",
            release_body(session_id, 1, rotated_a),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}


#[tokio::test]
async fn managed_surface_actions_require_current_owner_lease_for_take_and_complete() {
    let (state, session_id) = test_state().await;
    let owner = Uuid::new_v4();
    let (_, registration) = register_owner(state.clone(), owner).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "action-preview-open", registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "action-preview-open", 1, registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let (handshake_status, handshake_body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "preview_window",
            "preview-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(handshake_status, StatusCode::OK);
    assert_eq!(
        handshake_body,
        serde_json::json!([]),
        "first take establishes exact managed-surface authority fail-closed"
    );

    let queued = state
        .live
        .enqueue_action(session_id, Some("@save".into()), BridgeActionKind::Click)
        .await;

    let (take_status, take_body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "preview_window",
            "preview-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(take_status, StatusCode::OK, "surface take: {take_body}");
    assert_eq!(take_body.as_array().map(Vec::len), Some(1));
    assert_eq!(take_body[0]["id"], queued.id.to_string());

    let (_, rotated) = register_owner(state.clone(), owner).await;
    let (stale_status, stale_body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/complete",
        complete_action_body(session_id, queued.id, registration),
    )
    .await;
    assert_eq!(stale_status, StatusCode::CONFLICT);
    assert_eq!(stale_body["error"], "surface_owner_lease_mismatch");

    let (complete_status, complete_body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/complete",
        complete_action_body(session_id, queued.id, rotated),
    )
    .await;
    assert_eq!(
        complete_status,
        StatusCode::NO_CONTENT,
        "current lease must complete exact inflight action: {complete_body}"
    );

    let (repeat_status, repeat_body) = send(
        state,
        "/v1/runtime/resources/surfaces/actions/complete",
        complete_action_body(session_id, queued.id, rotated),
    )
    .await;
    assert_eq!(repeat_status, StatusCode::CONFLICT);
    assert_eq!(repeat_body["error"], "surface_action_not_inflight");
}

#[tokio::test]
async fn preview_surface_transition_retires_workspace_actions_and_becomes_primary() {
    let (state, session_id) = test_state().await;
    let owner = Uuid::new_v4();
    let (_, registration) = register_owner(state.clone(), owner).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "workspace-open", registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_workspace_body(session_id, "workspace-open", 1, registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let (workspace_handshake, workspace_empty) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "workspace_child",
            "workspace-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(workspace_handshake, StatusCode::OK);
    assert_eq!(workspace_empty, serde_json::json!([]));

    let stale_workspace_action = state
        .live
        .enqueue_action(session_id, None, BridgeActionKind::Focus)
        .await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "preview-open", registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "preview-open", 1, registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let (workspace_status, workspace_body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "workspace_child",
            "workspace-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(workspace_status, StatusCode::CONFLICT);
    assert_eq!(
        workspace_body["error"],
        "surface_not_primary_action_authority"
    );

    let (preview_handshake, preview_empty) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "preview_window",
            "preview-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(preview_handshake, StatusCode::OK);
    assert_eq!(
        preview_empty,
        serde_json::json!([]),
        "primary transition must retire all public actions from the previous surface authority"
    );
    assert!(
        state
            .live
            .request_action_cancellation(session_id, stale_workspace_action.id)
            .await
            .is_none(),
        "retired action must also leave cancellation authority"
    );

    let fresh_preview_action = state
        .live
        .enqueue_action(session_id, None, BridgeActionKind::Focus)
        .await;
    let (preview_status, preview_body) = send(
        state,
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "preview_window",
            "preview-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(preview_status, StatusCode::OK, "preview take: {preview_body}");
    assert_eq!(preview_body.as_array().map(Vec::len), Some(1));
    assert_eq!(preview_body[0]["id"], fresh_preview_action.id.to_string());
}

#[tokio::test]
async fn managed_surface_action_authority_never_replaces_native_provider_observation() {
    let (state, session_id) = test_state().await;
    let owner = Uuid::new_v4();
    let (_, registration) = register_owner(state.clone(), owner).await;

    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve_body(session_id, "provider-safe-preview", registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            state.clone(),
            "/v1/runtime/resources/surfaces/activate",
            activate_body(session_id, "provider-safe-preview", 1, registration),
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let provider = ProviderIncarnationRef::from("provider:native:test");
    let target = TargetIncarnationRef::from("target:native:test");
    state
        .live
        .bind_provider_observation(ProviderObservationBinding {
            session_id,
            generation: 11,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            initial_continuity: EventContinuityState::Continuous,
            sequence_baseline: Some(3),
        })
        .await
        .expect("bind independent native provider observation");

    let (status, body) = send(
        state.clone(),
        "/v1/runtime/resources/surfaces/actions/take",
        action_surface_body(
            session_id,
            "preview_window",
            "preview-owner-fence",
            1,
            registration,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "managed surface handshake: {body}");
    assert_eq!(body, serde_json::json!([]));

    let current = state
        .live
        .observation_status(session_id)
        .await
        .expect("native observation must remain bound");
    assert_eq!(current.provider_incarnation_ref, provider);
    assert_eq!(current.target_incarnation_ref, target);
    assert_eq!(current.event_continuity, EventContinuityState::Continuous);
    assert_eq!(current.last_seq, Some(3));
}
