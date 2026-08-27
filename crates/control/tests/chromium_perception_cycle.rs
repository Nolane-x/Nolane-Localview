#![recursion_limit = "256"]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{configure_chromium_executor_for_sessions, router, ControlState};
use localview_evidence::{EvidenceKind, EvidenceStore, UncertaintyClass};
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
use localview_observation::ObservationBus;
use localview_protocol::{Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind};
use localview_sessions::SessionManager;
use serde_json::Value;
use tower::ServiceExt;
use uuid::Uuid;

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn test_dir(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "localview-control-chromium-{name}-{}-{}",
        std::process::id(),
        nonce()
    ))
}

fn compile_fixture(name: &str, source: &str) -> (PathBuf, PathBuf) {
    let root = test_dir(name);
    fs::create_dir_all(&root).expect("fixture root");
    let source_path = root.join("fixture.rs");
    fs::write(&source_path, source).expect("fixture source");
    let executable = root.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    });
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .arg(&source_path)
        .arg("-O")
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("invoke rustc for deterministic fake Chromium");
    assert!(status.success(), "fake Chromium fixture must compile");
    (root, executable)
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
            cwd: Some("/tmp/localview-chromium-cycle".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Chromium Cycle".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state(executable: PathBuf, profile_root: PathBuf) -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
    let session_id = reconcile.created[0];
    configure_chromium_executor_for_sessions(&sessions, executable, profile_root);
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

async fn seed_semantic_and_layout(state: &ControlState, session_id: Uuid, route: &str) {
    state
        .live
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![
                ObserverEvent {
                    seq: 1,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::SemanticSnapshot,
                    reference: None,
                    route: Some(route.into()),
                    payload: serde_json::json!({"version": 1}),
                },
                ObserverEvent {
                    seq: 2,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Layout,
                    reference: None,
                    route: Some(route.into()),
                    payload: serde_json::json!({"verified": true}),
                },
            ],
        })
        .await;
}

fn cycle_body() -> Value {
    serde_json::json!({
        "budget": {
            "latency_ms": 5_000,
            "text_tokens": 800,
            "image_regions": 0,
            "chromium_spawns": 1
        },
        "deep_mode": false,
        "compatibility_requested": true,
        "target": "@save",
        "revision": "rev-chromium"
    })
}

async fn send(
    state: ControlState,
    method: Method,
    uri: String,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, "Bearer test-token");
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
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn assert_empty_dir(path: &Path) {
    let mut entries = fs::read_dir(path).expect("profile root");
    assert!(
        entries.next().is_none(),
        "Tier-3 Chromium must leave no persistent profile behind"
    );
}

#[tokio::test]
async fn planner_authorized_chromium_executes_once_retains_safe_evidence_and_replans_to_noop() {
    let source = r#"
fn main() {
    println!("<html><body>compatibility-ok</body></html>");
    eprintln!("fake chromium diagnostic");
}
"#;
    let (fixture_root, executable) = compile_fixture("cycle", source);
    let profile_root = test_dir("profiles");
    fs::create_dir_all(&profile_root).expect("profile root");
    let (state, session_id) = test_state(executable, profile_root.clone()).await;
    let route = "http://127.0.0.1:5173/settings";
    seed_semantic_and_layout(&state, session_id, route).await;

    let (status, body) = send(
        state.clone(),
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(cycle_body()),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "unexpected response: {body}");
    assert_eq!(body["completion"], "no_op");
    assert_eq!(body["steps"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        body["steps"][0]["plan"]["actions"][0]["action"]["kind"],
        "chromium_escalation"
    );
    assert_eq!(
        body["steps"][0]["execution"]["kind"],
        "chromium_compatibility"
    );
    assert_eq!(body["steps"][0]["execution"]["target"], route);
    assert_eq!(body["steps"][0]["execution"]["exit_code"], 0);
    assert_eq!(body["steps"][0]["execution"]["usage"]["chromium_spawns"], 1);
    assert_eq!(body["usage"]["chromium_spawns"], 1);
    assert!(
        body["steps"][0]["execution"].get("stdout").is_none(),
        "raw DOM/stdout must not be copied into the control response"
    );
    assert_empty_dir(&profile_root);

    let retained = state.evidence.recent_for_session(session_id, 64).await;
    let chromium = retained
        .iter()
        .rev()
        .find(|evidence| evidence.provenance.source == "chromium-compatibility")
        .expect("successful Tier-3 execution must retain bounded compatibility evidence");
    assert_eq!(chromium.kind, EvidenceKind::Visual);
    assert_eq!(chromium.provenance.engine.as_deref(), Some("chromium"));
    assert_eq!(chromium.provenance.revision.as_deref(), Some("rev-chromium"));
    assert_eq!(chromium.uncertainty, UncertaintyClass::Observed);
    assert!(chromium.confidence >= 0.999);
    assert!(!chromium.secret_taint);
    assert_eq!(chromium.payload["target"], route);
    assert_eq!(chromium.payload["exit_code"], 0);
    assert!(chromium.payload.get("stdout").is_none());
    assert!(chromium.payload["stdout_total_bytes"].as_u64().unwrap_or(0) > 0);

    let (plan_status, next_plan) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/plan"),
        Some(cycle_body()),
    )
    .await;
    assert_eq!(plan_status, StatusCode::OK);
    assert_eq!(next_plan["plan"]["actions"].as_array().map(Vec::len), Some(0));
    assert_eq!(next_plan["signals"]["browser_specific_suspicion"], false);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn caller_cannot_supply_a_chromium_executable_or_pre_authorized_probe() {
    let source = "fn main() {}";
    let (fixture_root, executable) = compile_fixture("caller", source);
    let profile_root = test_dir("caller-profiles");
    fs::create_dir_all(&profile_root).expect("profile root");
    let (state, session_id) = test_state(executable.clone(), profile_root.clone()).await;
    seed_semantic_and_layout(&state, session_id, "http://127.0.0.1:5173/").await;

    let mut body = cycle_body();
    body["chromium_executable"] = Value::String(executable.display().to_string());
    body["budget_escalation_reason"] = Value::String("browser_specific_suspicion".into());

    let (status, _) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}

#[tokio::test]
async fn browser_specific_budget_escalation_preserves_a_bounded_chromium_runtime_window() {
    let source = r#"
use std::{thread, time::Duration};
fn main() {
    thread::sleep(Duration::from_millis(120));
    println!("<html><body>delayed-compatibility-ok</body></html>");
}
"#;
    let (fixture_root, executable) = compile_fixture("escalated", source);
    let profile_root = test_dir("escalated-profiles");
    fs::create_dir_all(&profile_root).expect("profile root");
    let (state, session_id) = test_state(executable, profile_root.clone()).await;
    seed_semantic_and_layout(
        &state,
        session_id,
        "http://127.0.0.1:5173/compatibility",
    )
    .await;

    let mut body = cycle_body();
    body["budget"]["latency_ms"] = Value::from(40);

    let (status, response) = send(
        state,
        Method::POST,
        format!("/v1/sessions/{session_id}/perception/cycle"),
        Some(body),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "planner-authorized browser escalation must not be converted into a tiny executor timeout: {response}"
    );
    assert_eq!(response["completion"], "no_op");
    assert_eq!(response["usage"]["chromium_spawns"], 1);
    assert!(response["usage"]["latency_ms"].as_u64().unwrap_or(0) > 40);
    assert_eq!(response["budget_decision"]["status"], "escalated");
    assert_eq!(
        response["budget_decision"]["budget_escalation_reason"],
        "browser_specific_suspicion"
    );
    assert_empty_dir(&profile_root);

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
}
