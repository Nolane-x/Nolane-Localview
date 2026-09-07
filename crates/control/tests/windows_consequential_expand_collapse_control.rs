use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::Body,
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

fn discovered() -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: 5175,
                scheme: "http".into(),
            },
            pid: Some(79),
            process_name: Some("vite".into()),
            command: Some("vite".into()),
            cwd: Some("/tmp/windows-expand-collapse-control".into()),
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

fn body(extra: Option<(&str, &str)>) -> String {
    let mut value = serde_json::json!({
        "element_ref": {
            "provider_family": "windows_uia",
            "provider_incarnation_ref": "provider:windows-uia:expand-collapse-http-test",
            "target_incarnation_ref": "target:windows:expand-collapse-http-test",
            "opaque_provider_element_id": "uia-runtime:[4]",
            "semantic_locator_hints": ["class=ComboBox"],
            "parent_surface_ref": "window:expand-collapse-http-test",
            "acquisition_cut_ref": "cut:expand-collapse-http-test:1",
            "realization": "realized_current",
            "lifetime_profile_revision": "windows-uia-lifetime-v1"
        },
        "expected_postcondition_contract_refs": [
            "lvpc:native-semantic:v1:{\"expectation\":\"present\",\"matcher\":{\"class_name\":\"ComboBox\"}}"
        ]
    });
    if let Some((key, value_text)) = extra {
        value[key] = serde_json::Value::String(value_text.into());
    }
    value.to_string()
}

async fn post(app: axum::Router, session_id: Uuid, operation: &str, body: String) -> StatusCode {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(format!(
                "/v1/sessions/{session_id}/windows-observe/consequential/{operation}/plan"
            ))
            .header(AUTHORIZATION, "Bearer test-token")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
    .status()
}

#[tokio::test]
async fn expand_and_collapse_routes_are_server_owned_and_reach_runtime_boundary() {
    for operation in ["expand", "collapse"] {
        let (app, session_id) = fixture().await;
        assert_eq!(
            post(app, session_id, operation, body(None)).await,
            StatusCode::NOT_IMPLEMENTED,
            "{operation} route must exist and fail closed only when the Windows runtime is unavailable"
        );
    }
}

#[tokio::test]
async fn client_cannot_forge_expand_collapse_pattern_or_execution_verb() {
    for operation in ["expand", "collapse"] {
        for forged_field in ["required_pattern", "dispatch_verb", "risk_class"] {
            let (app, session_id) = fixture().await;
            assert_eq!(
                post(
                    app,
                    session_id,
                    operation,
                    body(Some((forged_field, "expand_collapse"))),
                )
                .await,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{operation} must reject client-owned {forged_field} authority"
            );
        }
    }
}
