use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore, UncertaintyClass};
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
            cwd: Some("/tmp/localview-visual-diff".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Visual Diff".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, Uuid, EvidenceStore) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    let evidence = EvidenceStore::new(128);
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: evidence.clone(),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id, evidence)
}

async fn send(
    state: ControlState,
    method: Method,
    uri: String,
    body: Option<Value>,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = router(state)
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn viewport() -> Value {
    serde_json::json!({
        "css_width": 1280,
        "css_height": 720,
        "device_scale_factor": 1.0
    })
}

fn visual_payload(revision: &str, route: &str) -> Value {
    serde_json::json!({
        "artifact_id": "lv-0123456789abcdef",
        "pixel_width": 1280,
        "pixel_height": 720,
        "backend": "webview2",
        "route": route,
        "viewport": viewport(),
        "revision": revision,
        "captured_at_unix_ms": Utc::now().timestamp_millis(),
        "target": "viewport"
    })
}

fn diff_payload(mode: &str, ratio: f64, revision: &str, route: &str, parents: Vec<String>) -> Value {
    serde_json::json!({
        "route": route,
        "viewport": viewport(),
        "revision": revision,
        "captured_at_unix_ms": Utc::now().timestamp_millis(),
        "mode": mode,
        "changed_ratio": ratio,
        "visual_evidence_ids": parents
    })
}

async fn create_visual_parent(state: ControlState, session_id: Uuid, revision: &str, route: &str) -> String {
    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual"),
        Some(visual_payload(revision, route)),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "visual parent: {body}");
    body["evidence_id"]
        .as_str()
        .expect("evidence id")
        .to_owned()
}

#[tokio::test]
async fn visual_diff_requires_auth_and_a_known_session() {
    let (state, session_id, _) = test_state().await;
    let payload = diff_payload("unchanged", 0.0, "rev-a", "http://127.0.0.1:5173/", vec![]);

    let (status, _) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual-diff"),
        Some(payload.clone()),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{}/evidence/visual-diff", Uuid::new_v4()),
        Some(payload),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn baseline_reset_diff_is_bounded_contract_evidence_with_exact_visual_parent() {
    let (state, session_id, evidence) = test_state().await;
    let route = "http://127.0.0.1:5173/settings?token=private#fragment";
    let parent = create_visual_parent(state.clone(), session_id, "rev-a", route).await;

    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual-diff"),
        Some(diff_payload(
            "baseline_reset",
            1.0,
            "rev-a",
            route,
            vec![parent.clone()],
        )),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "diff response: {body}");

    let id = body["evidence_id"].as_str().expect("diff evidence id");
    let diff = evidence.get(id).await.expect("stored diff evidence");
    assert_eq!(diff.kind, EvidenceKind::Contract);
    assert_eq!(diff.provenance.source, "native-visual-diff");
    assert_eq!(diff.provenance.engine.as_deref(), Some("pixel-diff"));
    assert_eq!(diff.provenance.revision.as_deref(), Some("rev-a"));
    assert_eq!(diff.provenance.parent_ids, vec![parent]);
    assert_eq!(diff.uncertainty, UncertaintyClass::Observed);
    assert!(diff.confidence >= 0.999);
    assert!(!diff.secret_taint);
    assert_eq!(diff.payload["mode"], "baseline_reset");
    assert_eq!(diff.payload["changed_ratio"], 1.0);
    assert_eq!(diff.payload["baseline_comparable"], false);
    assert_eq!(diff.payload["route"], "http://127.0.0.1:5173/settings");
    let stored = diff.payload.to_string();
    assert!(!stored.contains("private"));
    assert!(!stored.contains("fragment"));
    assert!(!stored.contains("png"));
    assert!(!stored.contains("base64"));
}

#[tokio::test]
async fn unchanged_diff_can_be_retained_without_a_new_visual_artifact() {
    let (state, session_id, evidence) = test_state().await;
    let (status, body) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/evidence/visual-diff"),
        Some(diff_payload(
            "unchanged",
            0.0,
            "rev-a",
            "http://127.0.0.1:5173/settings",
            vec![],
        )),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "diff response: {body}");
    let diff = evidence
        .get(body["evidence_id"].as_str().expect("diff evidence id"))
        .await
        .expect("stored diff");
    assert_eq!(diff.payload["mode"], "unchanged");
    assert_eq!(diff.payload["baseline_comparable"], true);
    assert!(diff.provenance.parent_ids.is_empty());
}

#[tokio::test]
async fn diff_authority_rejects_incoherent_modes_and_uncorrelated_visual_parents() {
    let (state, session_id, evidence) = test_state().await;
    let route = "http://127.0.0.1:5173/settings";
    let parent = create_visual_parent(state.clone(), session_id, "rev-a", route).await;

    let cases = [
        diff_payload("unchanged", 0.1, "rev-a", route, vec![]),
        diff_payload("regions", 0.2, "rev-a", route, vec![]),
        diff_payload("viewport", 0.2, "rev-a", route, vec![Uuid::new_v4().to_string()]),
        diff_payload("viewport", 0.2, "rev-b", route, vec![parent.clone()]),
        diff_payload(
            "viewport",
            0.2,
            "rev-a",
            "http://127.0.0.1:5173/other",
            vec![parent.clone()],
        ),
    ];

    for payload in cases {
        let (status, _) = send(
            state.clone(),
            Method::POST,
            format!("/v1/sessions/{session_id}/evidence/visual-diff"),
            Some(payload),
            true,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let recent = evidence.recent_for_session(session_id, 20).await;
    assert_eq!(
        recent.iter().filter(|item| item.kind == EvidenceKind::Contract).count(),
        0
    );
}
