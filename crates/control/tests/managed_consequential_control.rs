#![recursion_limit = "256"]

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{
    ControlState, SurfaceRecoveryJournal, configure_managed_consequential_control_for_sessions,
    configure_surface_recovery_journal_for_sessions, router,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialRecoveryState, DispatchPreparationReceipt, LiveBridge,
};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, PrincipalRef,
    ProviderIncarnationRef, ServerKind, TargetIncarnationRef,
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

async fn test_state_with_consequential_path() -> (ControlState, Uuid, PathBuf) {
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

    let consequential_path = std::env::temp_dir().join(format!(
        "localview-r7-managed-consequential-{}.jsonl",
        Uuid::new_v4()
    ));
    let consequential = Arc::new(
        ConsequentialJournal::open(&consequential_path)
            .await
            .expect("open consequential journal"),
    );
    configure_managed_consequential_control_for_sessions(
        &state.sessions,
        Some(consequential),
    );
    (state, session_id, consequential_path)
}

async fn test_state() -> (ControlState, Uuid) {
    let (state, session_id, _) = test_state_with_consequential_path().await;
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

async fn get(state: ControlState, uri: &str) -> (StatusCode, Value) {
    let response = router(state)
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(uri)
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
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

fn post_dispatch_snapshot_payload() -> Value {
    serde_json::json!({
        "version": 2,
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
                },
                {
                    "ref": "@edead",
                    "tag": "status",
                    "role": "status",
                    "name": "Saved",
                    "rect": null,
                    "interactive": false,
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

async fn complete_surface_action_with_outcome(
    state: ControlState,
    session_id: Uuid,
    registration: Registration,
    action_id: Uuid,
    ok: bool,
    payload: Value,
) -> StatusCode {
    let mut body = surface_body(session_id, registration);
    body.as_object_mut().expect("completion body").insert(
        "result".into(),
        serde_json::json!({
            "action_id": action_id,
            "ok": ok,
            "error": (!ok).then_some("executor reported failure"),
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

async fn complete_surface_action(
    state: ControlState,
    session_id: Uuid,
    registration: Registration,
    action_id: Uuid,
    payload: Value,
) -> StatusCode {
    complete_surface_action_with_outcome(
        state,
        session_id,
        registration,
        action_id,
        true,
        payload,
    )
    .await
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
                    "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edead\"}"
                ]
            }),
        )
        .await
    });

    let mut snapshot_action_id = None;
    for _ in 0..100 {
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
            snapshot_action_id = Some(action_id);
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let snapshot_action_id =
        snapshot_action_id.expect("R6 plan must request a bounded fresh precondition snapshot");

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
        "pending_executor_completion"
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
        state.clone(),
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

    // Executor completion alone is not enough. R6 must request a fresh semantic
    // snapshot and prove the expected postcondition on that new cut.
    assert_eq!(
        complete_surface_action(
            state.clone(),
            session_id,
            registration,
            action_id,
            serde_json::json!({ "clicked": true }),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let mut post_snapshot_action_id = None;
    for _ in 0..100 {
        let (status, body) =
            take_surface_actions(state.clone(), session_id, registration).await;
        assert_eq!(status, StatusCode::OK);
        if let Some(action) = body.as_array().and_then(|actions| actions.first()) {
            let snapshot_id =
                Uuid::parse_str(action["id"].as_str().expect("post-dispatch snapshot id"))
                    .expect("canonical snapshot uuid");
            assert_eq!(action["action"]["type"], "snapshot");
            post_snapshot_action_id = Some(snapshot_id);
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let post_snapshot_action_id =
        post_snapshot_action_id.expect("R6 must enqueue a bounded fresh post-dispatch snapshot");
    assert_eq!(
        complete_surface_action(
            state.clone(),
            session_id,
            registration,
            post_snapshot_action_id,
            post_dispatch_snapshot_payload(),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let status_uri =
        format!("/v1/sessions/{session_id}/managed-consequential/{action_id}/status");
    let mut terminal = None;
    for _ in 0..100 {
        let (status, body) = get(state.clone(), &status_uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["terminal"] == true {
            terminal = Some(body);
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let terminal = terminal.expect("R6 reconciliation must reach a bounded terminal status");
    assert_eq!(terminal["postcondition_status"], "verified_expected");
    assert_eq!(terminal["fresh_snapshot_version"], 2);
    assert!(
        terminal["proof_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with(&format!("postcondition:{action_id}:")))
    );
}

#[tokio::test]
async fn ambiguous_executor_failure_still_requires_fresh_world_reconciliation() {
    let (state, session_id, consequential_path) =
        test_state_with_consequential_path().await;
    let registration = register_owner(state.clone(), Uuid::new_v4()).await;
    activate_preview(state.clone(), session_id, registration).await;

    assert_eq!(
        take_surface_actions(state.clone(), session_id, registration)
            .await
            .1,
        serde_json::json!([])
    );

    let plan_state = state.clone();
    let plan = tokio::spawn(async move {
        post(
            plan_state,
            &format!("/v1/sessions/{session_id}/managed-consequential/plan"),
            serde_json::json!({
                "reference": "@eabc123",
                "action": { "type": "click" },
                "expected_postcondition_contract_refs": [
                    "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edead\"}"
                ]
            }),
        )
        .await
    });

    let mut pre_snapshot_action_id = None;
    for _ in 0..100 {
        let (status, body) =
            take_surface_actions(state.clone(), session_id, registration).await;
        assert_eq!(status, StatusCode::OK);
        if let Some(action) = body.as_array().and_then(|actions| actions.first()) {
            pre_snapshot_action_id = Some(
                Uuid::parse_str(action["id"].as_str().expect("pre snapshot action id"))
                    .expect("pre snapshot uuid"),
            );
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let pre_snapshot_action_id =
        pre_snapshot_action_id.expect("plan must request a fresh precondition snapshot");
    assert_eq!(
        complete_surface_action(
            state.clone(),
            session_id,
            registration,
            pre_snapshot_action_id,
            fresh_snapshot_payload(),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let (plan_status, plan_body) = plan.await.expect("plan task");
    assert_eq!(plan_status, StatusCode::CREATED, "{plan_body}");
    let action_id =
        Uuid::parse_str(plan_body["action_id"].as_str().expect("planned action id"))
            .expect("planned action uuid");
    let confirmation_ref = Uuid::parse_str(
        plan_body["confirmation_ref"]
            .as_str()
            .expect("confirmation ref"),
    )
    .expect("confirmation uuid");

    assert_eq!(
        post(
            state.clone(),
            &format!(
                "/v1/sessions/{session_id}/managed-consequential/{action_id}/confirm"
            ),
            serde_json::json!({ "confirmation_ref": confirmation_ref }),
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );

    let (take_status, take_body) =
        take_surface_actions(state.clone(), session_id, registration).await;
    assert_eq!(take_status, StatusCode::OK);
    assert_eq!(take_body.as_array().map(Vec::len), Some(1));
    assert_eq!(take_body[0]["id"], action_id.to_string());

    // A negative executor acknowledgement cannot prove that no side effect
    // crossed the WebView boundary. Durable dispatch is classified ambiguous,
    // and R7 must still obtain a fresh post-dispatch observation.
    assert_eq!(
        complete_surface_action_with_outcome(
            state.clone(),
            session_id,
            registration,
            action_id,
            false,
            serde_json::json!({ "executor_ack": "failed_after_possible_click" }),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let mut post_snapshot_action_id = None;
    for _ in 0..100 {
        let (status, body) =
            take_surface_actions(state.clone(), session_id, registration).await;
        assert_eq!(status, StatusCode::OK);
        if let Some(action) = body.as_array().and_then(|actions| actions.first()) {
            assert_eq!(action["action"]["type"], "snapshot");
            post_snapshot_action_id = Some(
                Uuid::parse_str(action["id"].as_str().expect("post snapshot action id"))
                    .expect("post snapshot uuid"),
            );
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let post_snapshot_action_id = post_snapshot_action_id
        .expect("ambiguous executor failure must still request fresh reconciliation");
    assert_eq!(
        complete_surface_action(
            state.clone(),
            session_id,
            registration,
            post_snapshot_action_id,
            post_dispatch_snapshot_payload(),
        )
        .await,
        StatusCode::NO_CONTENT
    );

    let status_uri =
        format!("/v1/sessions/{session_id}/managed-consequential/{action_id}/status");
    let mut terminal = None;
    for _ in 0..100 {
        let (status, body) = get(state.clone(), &status_uri).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["terminal"] == true {
            terminal = Some(body);
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    let terminal = terminal.expect("ambiguous dispatch reconciliation must terminate");
    assert_eq!(terminal["postcondition_status"], "verified_expected");
    assert!(
        terminal["proof_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with(&format!("postcondition:{action_id}:")))
    );
    assert_eq!(
        terminal["detail"],
        "executor_reported_failure_but_world_state_reconciled"
    );

    let durable = ConsequentialJournal::open(&consequential_path)
        .await
        .expect("reopen durable journal after verified reconciliation");
    assert_eq!(
        durable.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::Committed),
        "verified managed world state must be durably committed"
    );

    configure_managed_consequential_control_for_sessions(
        &state.sessions,
        Some(Arc::new(durable)),
    );
    let (restart_status, restart_body) = get(state, &status_uri).await;
    assert_eq!(restart_status, StatusCode::OK, "{restart_body}");
    assert_eq!(restart_body["durable_recovery"], true);
    assert_eq!(restart_body["durable_recovery_state"], "committed");
    assert_eq!(restart_body["postcondition_status"], "verified_expected");
    assert_eq!(restart_body["terminal"], true);
    assert_eq!(restart_body["retry_same_confirmation_allowed"], false);
    assert!(
        restart_body["proof_ref"]
            .as_str()
            .is_some_and(|value| value.starts_with(&format!("postcondition:{action_id}:")))
    );
}

#[tokio::test]
async fn durable_managed_status_survives_process_restart_without_restoring_retry_authority() {
    let (state, session_id) = test_state().await;
    let path = std::env::temp_dir().join(format!(
        "localview-r7-managed-restart-status-{}.jsonl",
        Uuid::new_v4()
    ));
    let journal = ConsequentialJournal::open(&path)
        .await
        .expect("open restart journal");
    let action_id = Uuid::new_v4();
    let provider_incarnation_ref =
        ProviderIncarnationRef::from("provider:managed-webview:{\"restart\":1}");
    let target_incarnation_ref =
        TargetIncarnationRef::from("target:managed-webview:{\"restart\":1}");
    let envelope = CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: action_id,
        session_id,
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:test:decision"),
            acting_principal_ref: PrincipalRef::from("principal:test:managed-webview"),
            authorization_revision: "authorization:test:managed-restart".into(),
            precondition_snapshot_cut_ref: "cut:test:managed-before-restart".into(),
            provider_incarnation_ref: provider_incarnation_ref.clone(),
            target_incarnation_ref: target_incarnation_ref.clone(),
            risk_class: ActionRiskClass::Unknown,
            idempotency_class: ActionIdempotencyClass::Unknown,
            expected_postcondition_contract_refs: vec![
                "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edead\"}"
                    .into(),
            ],
        },
    };
    journal
        .record_intent_admitted(envelope.clone())
        .await
        .expect("durable intent");
    let authorization = journal
        .record_authorization(
            action_id,
            envelope.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .expect("durable authorization");
    let prepared = journal
        .record_dispatch_prepared(
            action_id,
            DispatchPreparationReceipt {
                receipt_ref: format!("prepared:managed-restart:{action_id}"),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: envelope
                    .metadata
                    .precondition_snapshot_cut_ref
                    .clone(),
                provider_incarnation_ref,
                target_incarnation_ref,
            },
        )
        .await
        .expect("durable prepared state");
    drop(prepared);
    drop(journal);

    // Reopening the journal and reconfiguring the control handle models a new
    // daemon process: all confirmation, prepared-capability, execution-permit
    // and process-local reconciliation maps start empty.
    let reopened = Arc::new(
        ConsequentialJournal::open(&path)
            .await
            .expect("reopen restart journal"),
    );
    assert_eq!(
        reopened.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );
    configure_managed_consequential_control_for_sessions(
        &state.sessions,
        Some(reopened),
    );

    let (status, body) = get(
        state,
        &format!(
            "/v1/sessions/{session_id}/managed-consequential/{action_id}/status"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["durable_recovery"], true);
    assert_eq!(body["durable_recovery_state"], "dispatch_prepared");
    assert_eq!(body["postcondition_status"], "reconciliation_required");
    assert_eq!(body["terminal"], true);
    assert_eq!(body["retry_same_confirmation_allowed"], false);
    assert_eq!(
        body["detail"],
        "durable_recovery_requires_original_post_dispatch_lineage"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn managed_consequential_plan_rejects_non_web_and_duplicate_postconditions() {
    let (state, session_id) = test_state().await;
    let native = "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"name\":\"Done\"}}";
    let web = "lvpc:web-semantic:v1:{\"expectation\":\"present\",\"ref\":\"@edead\"}";

    let (native_status, native_body) = post(
        state.clone(),
        &format!("/v1/sessions/{session_id}/managed-consequential/plan"),
        serde_json::json!({
            "reference": "@eabc123",
            "action": { "type": "click" },
            "expected_postcondition_contract_refs": [native]
        }),
    )
    .await;
    assert_eq!(native_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        native_body["error"],
        "managed_consequential_invalid_postcondition_contract"
    );

    let (duplicate_status, duplicate_body) = post(
        state,
        &format!("/v1/sessions/{session_id}/managed-consequential/plan"),
        serde_json::json!({
            "reference": "@eabc123",
            "action": { "type": "focus" },
            "expected_postcondition_contract_refs": [web, web]
        }),
    )
    .await;
    assert_eq!(duplicate_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        duplicate_body["error"],
        "managed_consequential_invalid_postcondition_contract"
    );
}
