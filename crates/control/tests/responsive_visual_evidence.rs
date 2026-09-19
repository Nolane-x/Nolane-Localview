use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore};
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::{json, Value};
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
            cwd: Some("/tmp/localview-responsive-control-test".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Responsive Control Test".into()),
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
        live: LiveBridge::default(),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

fn valid_body() -> Value {
    json!({
        "artifact_id": "lv-0123456789abcdef",
        "route": "http://127.0.0.1:5173/page?secret=no#frag",
        "revision": "rev-responsive",
        "captured_at_unix_ms": Utc::now().timestamp_millis(),
        "contact_sheet_pixel_width": 1440,
        "contact_sheet_pixel_height": 1484,
        "viewports": [
            {
                "preset": "mobile_s",
                "css_width": 320,
                "css_height": 568,
                "device_scale_factor": 1.0,
                "pixel_width": 320,
                "pixel_height": 568,
                "sheet_x": 0,
                "sheet_y": 0
            },
            {
                "preset": "desktop",
                "css_width": 1440,
                "css_height": 900,
                "device_scale_factor": 1.0,
                "pixel_width": 1440,
                "pixel_height": 900,
                "sheet_x": 0,
                "sheet_y": 584
            }
        ]
    })
}

async fn post(
    state: ControlState,
    session_id: Uuid,
    authorized: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!(
            "/v1/sessions/{session_id}/evidence/visual-responsive"
        ))
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn responsive_visual_evidence_requires_auth_and_live_session() {
    let (state, session_id) = test_state().await;

    let (status, _) = post(state.clone(), session_id, false, valid_body()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = post(state, Uuid::new_v4(), true, valid_body()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn valid_responsive_contact_sheet_registers_one_visual_record() {
    let (state, session_id) = test_state().await;
    let (status, response) = post(state.clone(), session_id, true, valid_body()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(response["evidence_id"].as_str().is_some());

    let recent = state.evidence.recent_for_session(session_id, 8).await;
    assert_eq!(recent.len(), 1);
    let visual = &recent[0];
    assert_eq!(visual.kind, EvidenceKind::Visual);
    assert_eq!(
        visual.region.as_deref(),
        Some("responsive_contact_sheet")
    );
    assert_eq!(visual.provenance.source, "native-capture");
    assert_eq!(
        visual.provenance.engine.as_deref(),
        Some("responsive-contact-sheet")
    );
    assert_eq!(
        visual.payload["target"],
        Value::String("responsive_contact_sheet".into())
    );
    assert_eq!(
        visual.payload["route"],
        Value::String("http://127.0.0.1:5173/page".into())
    );
    assert_eq!(visual.payload["viewports"].as_array().unwrap().len(), 2);

    let serialized = serde_json::to_string(&visual.payload).unwrap();
    for forbidden in [
        "freeze_token",
        "selectors",
        "cookies",
        "local_storage",
        "dom_text",
        "mask_selectors",
        "source_contents",
    ] {
        assert!(!serialized.contains(forbidden));
    }
}

#[tokio::test]
async fn responsive_schema_rejects_unknown_fields_duplicates_and_noncanonical_order() {
    let (state, session_id) = test_state().await;

    let mut unknown = valid_body();
    unknown["caller_width"] = json!(999);
    let (status, _) = post(state.clone(), session_id, true, unknown).await;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
        "deny_unknown_fields must reject caller authority"
    );

    let mut duplicate = valid_body();
    duplicate["viewports"][1]["preset"] = json!("mobile_s");
    duplicate["viewports"][1]["css_width"] = json!(320);
    duplicate["viewports"][1]["css_height"] = json!(568);
    let (status, _) = post(state.clone(), session_id, true, duplicate).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut reversed = valid_body();
    reversed["viewports"].as_array_mut().unwrap().reverse();
    let (status, _) = post(state, session_id, true, reversed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn responsive_schema_rejects_dimension_scale_and_placement_drift() {
    let (state, session_id) = test_state().await;

    let mut wrong_css = valid_body();
    wrong_css["viewports"][0]["css_width"] = json!(321);
    let (status, _) = post(state.clone(), session_id, true, wrong_css).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut wrong_scale = valid_body();
    wrong_scale["viewports"][0]["device_scale_factor"] = json!(2.0);
    let (status, _) = post(state.clone(), session_id, true, wrong_scale).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut overlap = valid_body();
    overlap["viewports"][1]["sheet_y"] = json!(100);
    let (status, _) = post(state.clone(), session_id, true, overlap).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut outside = valid_body();
    outside["contact_sheet_pixel_height"] = json!(1000);
    let (status, _) = post(state, session_id, true, outside).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn responsive_schema_rejects_non_loopback_and_unbounded_revision() {
    let (state, session_id) = test_state().await;

    let mut external = valid_body();
    external["route"] = json!("https://example.com/");
    let (status, _) = post(state.clone(), session_id, true, external).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut revision = valid_body();
    revision["revision"] = json!("x".repeat(513));
    let (status, _) = post(state, session_id, true, revision).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
