use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::AUTHORIZATION},
};
use chrono::Utc;
use localview_control::{
    ControlState, configure_windows_consequential_control_for_sessions,
    configure_windows_observe_runtime_for_sessions, router,
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

const SENTINEL: &str = "localview-task8-set-value-secret-6d6f8f22";

fn discovered() -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: 5176,
                scheme: "http".into(),
            },
            pid: Some(80),
            process_name: Some("vite".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/windows-set-value-control".into()),
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

fn element_ref_json() -> serde_json::Value {
    serde_json::json!({
        "provider_family": "windows_uia",
        "provider_incarnation_ref": "provider:windows-uia:set-value-http-test",
        "target_incarnation_ref": "target:windows:set-value-http-test",
        "opaque_provider_element_id": "uia-runtime:[5]",
        "semantic_locator_hints": ["automation_id=value-input"],
        "parent_surface_ref": "window:set-value-http-test",
        "acquisition_cut_ref": "cut:set-value-http-test:1",
        "realization": "realized_current",
        "lifetime_profile_revision": "windows-uia-lifetime-v1"
    })
}

async fn post(app: axum::Router, session_id: Uuid, body: serde_json::Value) -> (StatusCode, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/v1/sessions/{session_id}/windows-observe/consequential/set-value/plan"
                ))
                .header(AUTHORIZATION, "Bearer test-token")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("read bounded SetValue response body");
    let body = String::from_utf8(bytes.to_vec()).expect("SetValue response is UTF-8");
    (status, body)
}

fn assert_private_response(body: &str) {
    assert!(
        !body.contains(SENTINEL),
        "SetValue HTTP response must never echo process-local plaintext"
    );
    assert!(
        !body.contains("commitment_digest"),
        "SetValue HTTP response must never expose durable HMAC material"
    );
}

#[tokio::test]
async fn valid_replace_value_route_reaches_runtime_boundary_without_echoing_plaintext() {
    let (app, session_id) = fixture().await;
    let (status, body) = post(
        app,
        session_id,
        serde_json::json!({
            "mode": "replace_value",
            "element_ref": element_ref_json(),
            "value": SENTINEL,
        }),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::NOT_IMPLEMENTED,
        "a structurally valid SetValue plan must reach the existing unavailable runtime boundary"
    );
    assert_private_response(&body);
}

#[tokio::test]
async fn client_cannot_forge_server_owned_set_value_authority() {
    for forged_field in [
        "required_pattern",
        "dispatch_verb",
        "risk_class",
        "commitment_digest",
    ] {
        let (app, session_id) = fixture().await;
        let mut body = serde_json::json!({
            "mode": "replace_value",
            "element_ref": element_ref_json(),
            "value": SENTINEL,
        });
        body[forged_field] = serde_json::Value::String("forged".into());
        let (status, response_body) = post(app, session_id, body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "caller-owned {forged_field} must be rejected before runtime lookup"
        );
        assert_private_response(&response_body);
    }
}

#[tokio::test]
async fn clear_value_rejects_a_value_field_before_runtime_lookup() {
    let (app, session_id) = fixture().await;
    let (status, body) = post(
        app,
        session_id,
        serde_json::json!({
            "mode": "clear_value",
            "element_ref": element_ref_json(),
            "value": SENTINEL,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_private_response(&body);
}

#[tokio::test]
async fn replace_value_rejects_payload_larger_than_sixteen_kib_before_runtime_lookup() {
    let (app, session_id) = fixture().await;
    let oversized = "x".repeat(16 * 1024 + 1);
    let (status, body) = post(
        app,
        session_id,
        serde_json::json!({
            "mode": "replace_value",
            "element_ref": element_ref_json(),
            "value": oversized,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!body.contains(&"x".repeat(256)));
    assert!(!body.contains("commitment_digest"));
}

#[tokio::test]
async fn replace_value_rejects_u0000_before_runtime_lookup_without_echoing_plaintext() {
    let (app, session_id) = fixture().await;
    let nul_payload = format!("{SENTINEL}\0tail");
    let (status, body) = post(
        app,
        session_id,
        serde_json::json!({
            "mode": "replace_value",
            "element_ref": element_ref_json(),
            "value": nul_payload,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_private_response(&body);
}
