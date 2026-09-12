#![recursion_limit = "256"]

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use chrono::Utc;
use localview_control::{
    ControlState, configure_chromium_executor_for_sessions, router,
    runtime_resource_governor_for_sessions,
};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_resource_governor::ResourceWorkKind;
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
        "localview-control-chromium-authority-{name}-{}-{}",
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
            cwd: Some("/tmp/localview-chromium-authority".into()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("Chromium Authority".into()),
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

async fn seed_semantic_and_layout(state: &ControlState, session_id: Uuid) {
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
                    route: Some("http://127.0.0.1:5173/settings".into()),
                    payload: serde_json::json!({"version": 1}),
                },
                ObserverEvent {
                    seq: 2,
                    captured_at: Utc::now(),
                    kind: ObserverEventKind::Layout,
                    reference: None,
                    route: Some("http://127.0.0.1:5173/settings".into()),
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
        "revision": "rev-chromium-authority"
    })
}

async fn send(state: ControlState, session_id: Uuid) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/v1/sessions/{session_id}/perception/cycle"))
        .header(header::AUTHORIZATION, "Bearer test-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(cycle_body().to_string()))
        .expect("request");
    let response = router(state).oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024)
        .await
        .expect("bounded response");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn session_cleanup_cannot_erase_a_live_chromium_process_authority() {
    let marker_root = test_dir("marker");
    fs::create_dir_all(&marker_root).expect("marker root");
    let marker = marker_root.join("spawned");
    let marker_literal = format!("{:?}", marker.to_string_lossy());
    let source = format!(
        "fn main() {{ std::fs::write({marker_literal}, b\"spawned\").unwrap(); std::thread::sleep(std::time::Duration::from_millis(500)); println!(\"<html>done</html>\"); }}"
    );
    let (fixture_root, executable) = compile_fixture("live-owner", &source);
    let profile_root = test_dir("profiles");
    fs::create_dir_all(&profile_root).expect("profile root");
    let (state, session_id) = test_state(executable, profile_root.clone()).await;
    seed_semantic_and_layout(&state, session_id).await;

    let first_state = state.clone();
    let first = tokio::spawn(async move { send(first_state, session_id).await });

    for _ in 0..200 {
        if marker.is_file() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        marker.is_file(),
        "fake Chromium must reach its live child body"
    );

    let governor = runtime_resource_governor_for_sessions(&state.sessions);
    assert_eq!(
        governor.release_session(&session_id.to_string()),
        0,
        "session pending-work cleanup must not forge the death of a spawned Chromium child"
    );
    assert!(
        governor
            .reserve(
                "other-session",
                "while-first-child-is-live",
                ResourceWorkKind::Chromium,
            )
            .is_err(),
        "the default one-Chromium budget must stay occupied while the real child is live"
    );

    let (status, body) = first.await.expect("cycle task");
    assert_eq!(status, StatusCode::OK, "unexpected response: {body}");

    assert!(
        governor
            .reserve(
                "other-session",
                "after-first-child-exits",
                ResourceWorkKind::Chromium,
            )
            .is_ok(),
        "child termination must drop its owner lease and reopen Chromium admission"
    );

    let _ = fs::remove_dir_all(fixture_root);
    let _ = fs::remove_dir_all(profile_root);
    let _ = fs::remove_dir_all(marker_root);
}
