use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{ControlState, router};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("localview-runtime-source-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temporary project root");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture");
    }

    fn map_json(&self, source: &str) -> String {
        json!({
            "version": 3,
            "sources": [source],
            "names": [],
            "sourcesContent": ["MUST-NOT-LEAK-SOURCE-CONTENT"],
            "mappings": "AAAA"
        })
        .to_string()
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn discovered(cwd: &Path) -> DiscoveredServer {
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
            cwd: Some(cwd.to_string_lossy().into_owned()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Runtime Source Correlation Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state(project: &TempProject) -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered(&project.root)], Utc::now())
        .await;
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

async fn post(
    state: ControlState,
    session_id: Uuid,
    authorized: bool,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session_id}/runtime-source/resolve"))
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }

    let response = router(state)
        .oneshot(
            builder
                .body(Body::from(
                    serde_json::to_vec(&body).expect("serialize runtime-source request"),
                ))
                .expect("build runtime-source request"),
        )
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn runtime_event(
    seq: u64,
    source: &str,
    line: Value,
    column: Value,
    marker: &str,
) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at: Utc::now(),
        kind: ObserverEventKind::RuntimeError,
        reference: None,
        route: Some(format!("/private-route?token={marker}")),
        payload: json!({
            "message": marker,
            "stack": format!("STACK-{marker}"),
            "source": source,
            "line": line,
            "column": column
        }),
    }
}

async fn ingest(state: &ControlState, session_id: Uuid, events: Vec<ObserverEvent>) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events,
        })
        .await;
}

fn assert_error(value: &Value, expected: &str) {
    assert_eq!(value.get("error").and_then(Value::as_str), Some(expected));
}

#[tokio::test]
async fn resolves_retained_runtime_error_without_leaking_observer_payload() {
    let project = TempProject::new();
    project.write("dist/app.js", "console.log('generated');");
    project.write("src/App.tsx", "export const App = () => null;");
    project.write("dist/app.js.map", project.map_json("../src/App.tsx"));

    let (state, session_id) = test_state(&project).await;
    ingest(
        &state,
        session_id,
        vec![runtime_event(
            7,
            "http://localhost:5173/dist/app.js?token=QUERY-MUST-NOT-LEAK#fragment",
            json!(1),
            json!(1),
            "RUNTIME-MUST-NOT-LEAK",
        )],
    )
    .await;

    let (status, value) = post(state, session_id, true, json!({ "event_seq": 7 })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["event_seq"], 7);
    assert_eq!(value["resolution"]["generated_file"], "dist/app.js");
    assert_eq!(value["resolution"]["map_file"], "dist/app.js.map");
    assert_eq!(value["resolution"]["generated_line"], 1);
    assert_eq!(value["resolution"]["generated_column"], 0);
    assert_eq!(value["resolution"]["source"]["file"], "src/App.tsx");
    assert_eq!(value["resolution"]["source"]["line"], 1);
    assert_eq!(value["resolution"]["source"]["column"], 0);

    let serialized = serde_json::to_string(&value).expect("serialize response");
    for forbidden in [
        "RUNTIME-MUST-NOT-LEAK",
        "QUERY-MUST-NOT-LEAK",
        "STACK-RUNTIME",
        "/private-route",
        "MUST-NOT-LEAK-SOURCE-CONTENT",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "runtime source response leaked forbidden marker: {forbidden}"
        );
    }
    assert!(!serialized.contains(&project.root.to_string_lossy().to_string()));
}

#[tokio::test]
async fn requires_auth_known_session_exact_event_and_non_authoritative_request() {
    let project = TempProject::new();
    let (state, session_id) = test_state(&project).await;

    let (status, value) = post(state.clone(), session_id, false, json!({ "event_seq": 1 })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_error(&value, "unauthorized");

    let (status, value) = post(
        state.clone(),
        Uuid::new_v4(),
        true,
        json!({ "event_seq": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&value, "session_not_found");

    ingest(
        &state,
        Uuid::new_v4(),
        vec![runtime_event(
            5,
            "http://localhost:5173/dist/app.js",
            json!(1),
            json!(1),
            "FOREIGN-SESSION",
        )],
    )
    .await;
    let (status, value) = post(state.clone(), session_id, true, json!({ "event_seq": 5 })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&value, "runtime_event_not_found");

    ingest(
        &state,
        session_id,
        vec![ObserverEvent {
            seq: 6,
            captured_at: Utc::now(),
            kind: ObserverEventKind::Console,
            reference: None,
            route: None,
            payload: json!({"message":"not runtime authority"}),
        }],
    )
    .await;
    let (status, value) = post(state.clone(), session_id, true, json!({ "event_seq": 6 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "runtime_event_kind_unsupported");

    let (status, _) = post(
        state,
        session_id,
        true,
        json!({
            "event_seq": 6,
            "generated_file": "dist/attacker.js",
            "generated_line": 1,
            "generated_column": 0
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn rejects_remote_mismatched_encoded_and_invalid_runtime_positions() {
    let project = TempProject::new();
    let (state, session_id) = test_state(&project).await;

    ingest(
        &state,
        session_id,
        vec![
            runtime_event(
                20,
                "http://example.com:5173/dist/app.js",
                json!(1),
                json!(1),
                "REMOTE",
            ),
            runtime_event(
                21,
                "http://localhost:9000/dist/app.js",
                json!(1),
                json!(1),
                "PORT",
            ),
            runtime_event(
                22,
                "https://localhost:5173/dist/app.js",
                json!(1),
                json!(1),
                "SCHEME",
            ),
            runtime_event(
                23,
                "http://localhost:5173/dist/%2e%2e/app.js",
                json!(1),
                json!(1),
                "ENCODED",
            ),
            runtime_event(
                24,
                "http://localhost:5173/dist/app.js",
                json!(0),
                json!(1),
                "LINE",
            ),
            runtime_event(
                25,
                "http://localhost:5173/dist/app.js",
                json!(1),
                json!(0),
                "COLUMN",
            ),
        ],
    )
    .await;

    for seq in [20_u64, 21, 22] {
        let (status, value) =
            post(state.clone(), session_id, true, json!({ "event_seq": seq })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_error(&value, "runtime_source_authority_mismatch");
    }

    let (status, value) = post(state.clone(), session_id, true, json!({ "event_seq": 23 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "runtime_source_unsupported");

    for seq in [24_u64, 25] {
        let (status, value) =
            post(state.clone(), session_id, true, json!({ "event_seq": seq })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_error(&value, "runtime_position_invalid");
    }
}
