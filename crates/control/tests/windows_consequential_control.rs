use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::Body,
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{
    configure_windows_consequential_control_for_sessions,
    configure_windows_observe_runtime_for_sessions, router, ControlState,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
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
            pid: Some(77),
            process_name: Some("vite".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/windows-consequential-control".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: None,
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn fixture() -> (axum::Router, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    configure_windows_observe_runtime_for_sessions(&sessions, None);
    configure_windows_consequential_control_for_sessions(&sessions, None);
    let app = router(ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(16),
        live: LiveBridge::new(32, 8),
        evidence: EvidenceStore::default(),
        paused: Arc::new(AtomicBool::new(false)),
    });
    (app, session_id)
}

fn valid_contract() -> String {
    "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"name\":\"Done\"}}"
        .into()
}

fn element_ref_json() -> serde_json::Value {
    serde_json::json!({
        "provider_family": "windows_uia",
        "provider_incarnation_ref": "provider:windows-uia:http-test",
        "target_incarnation_ref": "target:windows:http-test",
        "opaque_provider_element_id": "uia-runtime:[1]",
        "semantic_locator_hints": ["automation_id=submit"],
        "parent_surface_ref": "window:http-test",
        "acquisition_cut_ref": "cut:http-test:1",
        "realization": "realized_current",
        "lifetime_profile_revision": "windows-uia-lifetime-v1"
    })
}

fn plan_body(contract: String) -> String {
    serde_json::json!({
        "element_ref": element_ref_json(),
        "expected_postcondition_contract_refs": [contract]
    })
    .to_string()
}

#[tokio::test]
async fn consequential_plan_requires_control_bearer_auth() {
    let (app, session_id) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{session_id}/windows-observe/consequential/invoke/plan"
                ))
                .header("content-type", "application/json")
                .body(Body::from(plan_body(valid_contract())))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_session_is_rejected_before_consequential_runtime_lookup() {
    let (app, _) = fixture().await;
    let missing = Uuid::from_u128(0x9104);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{missing}/windows-observe/consequential/invoke/plan"
                ))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(plan_body(valid_contract())))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn existing_session_fails_closed_when_windows_runtime_is_unavailable() {
    let (app, session_id) = fixture().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{session_id}/windows-observe/consequential/invoke/plan"
                ))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(plan_body(valid_contract())))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn client_cannot_supply_risk_pattern_or_principal_authority() {
    let (app, session_id) = fixture().await;
    let body = serde_json::json!({
        "element_ref": element_ref_json(),
        "expected_postcondition_contract_refs": [valid_contract()],
        "risk_class": "s1_reversible_ui_state",
        "required_pattern": "invoke",
        "decision_principal_ref": "principal:forged"
    })
    .to_string();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{session_id}/windows-observe/consequential/invoke/plan"
                ))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn unknown_postcondition_version_is_rejected_before_runtime_lookup() {
    let (app, session_id) = fixture().await;
    let unsupported = "lvpc:native-semantic:v3:{\"comparison\":\"at_least\",\"count\":1,\"matcher\":{\"name\":\"Done\"}}".to_owned();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{session_id}/windows-observe/consequential/invoke/plan"
                ))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(plan_body(unsupported)))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
