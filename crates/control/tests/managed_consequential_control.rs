#![recursion_limit = "256"]

use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{
    ControlState, SurfaceRecoveryJournal, configure_surface_recovery_journal_for_sessions, router,
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
use tokio::time::sleep;
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Deserialize)]
struct Registration {
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
            cwd: Some("/tmp/localview-r5-managed-consequential".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("R5 Managed Consequential".into()),
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
        "localview-r5-managed-consequential-surface-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = Arc::new(
        SurfaceRecoveryJournal::open(journal_path)
            .await
            .expect("open surface recovery journal"),
    );
    configure_surface_recovery_journal_for_sessions(&state.sessions, Some(journal));
    (state, session_id)
}

async fn post(state: ControlState, uri: &str, body: Value) -> (StatusCode, Value) {
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
    let (status, value) = post(
        state,
        "/v1/runtime/resources/surfaces/owners/register",
        serde_json::json!({ "owner_instance_id": owner_instance_id }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_value(value).expect("registration")
}

fn owner_fields(registration: Registration) -> Value {
    serde_json::json!({
        "owner_instance_id": registration.owner_instance_id,
        "boot_epoch": registration.boot_epoch,
        "owner_lease_id": registration.owner_lease_id,
    })
}

fn surface_body(session_id: Uuid, registration: Registration) -> Value {
    let mut value = serde_json::json!({
        "session_id": session_id,
        "surface_kind": "preview_window",
        "label": "r5-preview",
        "incarnation": 1,
    });
    value
        .as_object_mut()
        .expect("surface body")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    value
}

async fn activate_preview(
    state: ControlState,
    session_id: Uuid,
    registration: Registration,
) {
    let mut reserve = serde_json::json!({
        "session_id": session_id,
        "request_id": "r5-preview-open",
    });
    reserve
        .as_object_mut()
        .expect("reserve body")
        .extend(owner_fields(registration).as_object().expect("owner fields").clone());
    assert_eq!(
        post(
            state.clone(),
            "/v1/runtime/resources/surfaces/reserve",
            reserve,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let mut activate = surface_body(session_id, registration);
    activate.as_object_mut().expect("activate body").extend(
        serde_json::json!({
            "request_id": "r5-preview-open",
            "visibility": "visible",
        })
        .as_object()
        .expect("activation fields")
        .clone(),
    );
    assert_eq!(
        post(
            state,
            "/v1/runtime/resources/surfaces/activate",
            activate,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

fn fresh_snapshot_payload() -> Value {
    serde_json::json!({
        "version": 1,
        "route": "/settings",
        "viewport": {
            "width": 1280,
            "height": 720
        },
        "semantic_tree": {
            "ref": "@e1",
            "tag": "main",
            "role": "main",
            "name": "Settings",
            "rect": {
                "x": 0.0,
                "y": 0.0,
                "width": 1280.0,
                "height": 720.0
            },
            "interactive": false,
            "attributes": {},
            "sourceHint": null,
            "children": [
                {
                    "ref": "@eabc123",
                    "tag": "button",
                    "role": "button",
                    "name": "Save",
                    "rect": {
                        "x": 20.0,
                        "y": 20.0,
                        "width": 100.0,
                        "height": 40.0
                    },
                    "interactive": true,
                    "attributes": {},
                    "sourceHint": null,
                    "children": []
                }
            ]
        }
    })
}

async fn take_surface_actions(
    state: ControlState,
    session_id: Uuid,
    registration: Registration,
) -> (StatusCode, Value) {
    post(
        state,
        "/v1/runtime/resources/surfaces/actions/take",
        surface_body(session_id, registration),
    )
    .await
}

async fn complete_surface_action(
    state: ControlState,
    session_id: Uuid,
    registration: Registration,
    action_id: Uuid,
    payload: Value,
) -> StatusCode {
    let mut body = surface_body(session_id, registration);
    body.as_object_mut().expect("completion body").insert(
        "result".into(),
        serde_json::json!({
            "action_id": action_id,
            "ok": true,
            "error": null,
            "payload": payload,
            "completed_at": Utc::now(),
        }),
    );
    post(
        state,
        "/v1/runtime/resources/surfaces/actions/complete",
        body,
    )
    .await
    .0
}

#[tokio::test]
async fn managed_consequential_action_requires_fresh_plan_and_one_shot_confirmation() {
    let (state, session_id) = test_state().await;
    let registration = register_owner(state.clone(), Uuid::new_v4()).await;
    activate_preview(state.clone(), session_id, registration).await;

    // Establish exact managed-surface executor/observation lineage before the
    // plan. No consequential work exists yet.
    let (status, body) =
        take_surface_actions(state.clone(), session_id, registration).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    let plan_state = state.clone();
    let plan = tokio::spawn(async move {
        post(
            plan_state,
            &format!("/v1/sessions/{session_id}/managed-consequential/plan"),
            serde_json::json!({
                "reference": "@eabc123",
                "action": { "type": "click" },
                "expected_postcondition_contract_refs": [
                    "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edone\"}"
                ]
            }),
        )
        .await
    });

    let snapshot_action_id = loop {
        let (status, body) =
            take_surface_actions(state.clone(), session_id, registration).await;
        assert_eq!(status, StatusCode::OK);
        if let Some(action) = body.as_array().and_then(|actions| actions.first()) {
            let action_id = Uuid::parse_str(
                action["id"]
                    .as_str()
                    .expect("fresh snapshot action id"),
            )
            .expect("canonical snapshot uuid");
            assert_eq!(action["action"]["type"], "snapshot");
            break action_id;
        }
        sleep(Duration::from_millis(10)).await;
    };

    assert_eq!(
        complete_surface_action(
            state.clone(),
            session_id,
            registration,
            snapshot_action_id,
            fresh_snapshot_payload(),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (plan_status, plan_body) = plan.await.expect("plan task");
    assert_eq!(plan_status, StatusCode::CREATED, "{plan_body}");
    assert_eq!(plan_body["dispatch_performed"], false);
    assert_eq!(
        plan_body["restart_restores_confirmation_authority"],
        false
    );
    let action_id = Uuid::parse_str(
        plan_body["action_id"]
            .as_str()
            .expect("planned action id"),
    )
    .expect("planned action uuid");
    let confirmation_ref = Uuid::parse_str(
        plan_body["confirmation_ref"]
            .as_str()
            .expect("confirmation ref"),
    )
    .expect("confirmation uuid");

    // Direct canonical binding is intentionally invisible to the managed
    // executor until explicit confirmation.
    let (status, body) =
        take_surface_actions(state.clone(), session_id, registration).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    // A wrong capability cannot consume or dispatch the plan.
    let (wrong_status, wrong_body) = post(
        state.clone(),
        &format!(
            "/v1/sessions/{session_id}/managed-consequential/{action_id}/confirm"
        ),
        serde_json::json!({ "confirmation_ref": Uuid::new_v4() }),
    )
    .await;
    assert_eq!(wrong_status, StatusCode::CONFLICT);
    assert_eq!(
        wrong_body["error"],
        "managed_consequential_confirmation_mismatch"
    );
    assert_eq!(
        take_surface_actions(state.clone(), session_id, registration)
            .await
            .1,
        serde_json::json!([])
    );

    let (confirm_status, confirm_body) = post(
        state.clone(),
        &format!(
            "/v1/sessions/{session_id}/managed-consequential/{action_id}/confirm"
        ),
        serde_json::json!({ "confirmation_ref": confirmation_ref }),
    )
    .await;
    assert_eq!(confirm_status, StatusCode::ACCEPTED, "{confirm_body}");
    assert_eq!(confirm_body["confirmation_consumed"], true);
    assert_eq!(
        confirm_body["postcondition_status"],
        "pending_fresh_reconciliation"
    );

    let (take_status, take_body) =
        take_surface_actions(state.clone(), session_id, registration).await;
    assert_eq!(take_status, StatusCode::OK);
    let actions = take_body.as_array().expect("queued actions");
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0]["id"],
        action_id.to_string(),
        "confirmed action must keep its canonical transport id"
    );
    assert_eq!(actions[0]["action"]["type"], "click");

    // Process-local confirmation is one-shot even before the first action result.
    let (replay_status, replay_body) = post(
        state,
        &format!(
            "/v1/sessions/{session_id}/managed-consequential/{action_id}/confirm"
        ),
        serde_json::json!({ "confirmation_ref": confirmation_ref }),
    )
    .await;
    assert_eq!(replay_status, StatusCode::CONFLICT);
    assert_eq!(
        replay_body["error"],
        "managed_consequential_confirmation_missing"
    );
}
