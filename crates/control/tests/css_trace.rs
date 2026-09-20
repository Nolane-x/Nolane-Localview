#![recursion_limit = "256"]

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
use localview_live_bridge::{BridgeActionKind, BridgeActionResult, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
use serde_json::Value;
use tower::ServiceExt;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("localview-css-source-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create temporary CSS project root");
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: impl AsRef<[u8]>) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create CSS fixture parent");
        }
        fs::write(path, contents).expect("write CSS fixture");
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn discovered_with_cwd(cwd: Option<&Path>) -> DiscoveredServer {
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
            cwd: cwd.map(|path| path.to_string_lossy().into_owned()),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some("CSS Trace Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

fn discovered() -> DiscoveredServer {
    discovered_with_cwd(Some(Path::new("/tmp/localview-css-trace-test")))
}

async fn test_state_with_cwd(cwd: Option<&Path>) -> (ControlState, uuid::Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered_with_cwd(cwd)], Utc::now())
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

async fn test_state() -> (ControlState, uuid::Uuid) {
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

fn raw_snapshot_payload() -> Value {
    serde_json::json!({
        "version": 12,
        "route": "http://127.0.0.1:5173/settings",
        "viewport": {"width": 1000, "height": 800, "dpr": 1.0},
        "semantic_tree": {
            "ref": "@root",
            "tag": "main",
            "role": "main",
            "name": null,
            "rect": {"x": 0.0, "y": 0.0, "width": 1000.0, "height": 800.0},
            "interactive": false,
            "attributes": {},
            "sourceHint": null,
            "style": null,
            "styleTrace": null,
            "children": [{
                "ref": "@save",
                "tag": "button",
                "role": "button",
                "name": "Save",
                "rect": {"x": 320.0, "y": 300.0, "width": 100.0, "height": 40.0},
                "interactive": true,
                "attributes": {"type": "button"},
                "sourceHint": null,
                "style": {
                    "display": "flex",
                    "paddingTop": "16px",
                    "color": "rgb(20, 20, 20)"
                },
                "styleTrace": {
                    "declarations": [
                        {
                            "source_kind": "inline_element",
                            "stylesheet_path": null,
                            "selector": null,
                            "property": "display",
                            "value": "flex",
                            "important": false
                        },
                        {
                            "source_kind": "same_origin_stylesheet",
                            "stylesheet_path": "src/button.css",
                            "selector": ".save",
                            "property": "padding-top",
                            "value": "16px",
                            "important": false
                        }
                    ],
                    "authorCascade": {
                        "scope": "supported_author_subset",
                        "coverage_complete": true,
                        "unresolved_properties": ["color"],
                        "winners": [{
                            "source_kind": "inline_element",
                            "stylesheet_path": null,
                            "selector": null,
                            "property": "display",
                            "value": "flex",
                            "important": false,
                            "specificity": [1, 0, 0, 0],
                            "source_order": 0
                        }]
                    }
                },
                "children": []
            }]
        }
    })
}

async fn complete_next_snapshot(state: ControlState, session_id: uuid::Uuid, payload: Value) {
    for _ in 0..120 {
        let actions = state.live.take_actions(session_id, 8).await;
        if let Some(action) = actions
            .into_iter()
            .find(|action| matches!(&action.action, BridgeActionKind::Snapshot))
        {
            let claimed = state
                .live
                .claim_action(session_id, action.id)
                .await
                .expect("style trace Snapshot action must become inflight");
            state
                .live
                .complete_action(
                    &claimed,
                    BridgeActionResult {
                        action_id: claimed.id,
                        ok: true,
                        error: None,
                        payload,
                        completed_at: Utc::now(),
                    },
                )
                .await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("style trace endpoint did not enqueue a Snapshot action");
}

async fn get_trace(
    state: ControlState,
    session_id: uuid::Uuid,
    reference: &str,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/style-trace/{reference}"));
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }
    let response = router(state)
        .oneshot(builder.body(Body::empty()).expect("style trace request"))
        .await
        .expect("control router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("bounded style trace body");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn fresh_style_trace_requires_authentication() {
    let (state, session_id) = test_state().await;
    let (status, body) = get_trace(state, session_id, "@save", false).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthorized");
}

#[tokio::test]
async fn fresh_style_trace_returns_only_selected_bounded_css_evidence() {
    let (state, session_id) = test_state().await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(executor_state, session_id, raw_snapshot_payload()).await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["reference"], "@save");
    assert_eq!(body["computed"]["display"], "flex");
    assert_eq!(body["computed"]["paddingTop"], "16px");
    assert_eq!(body["declarations"].as_array().map(Vec::len), Some(2));
    assert_eq!(body["declarations"][1]["stylesheet_path"], "src/button.css");
    assert_eq!(body["declarations"][1]["selector"], ".save");
    assert_eq!(body["author_cascade"]["scope"], "supported_author_subset");
    assert_eq!(body["author_cascade"]["coverage_complete"], true);
    assert_eq!(body["author_cascade"]["unresolved_properties"][0], "color");
    assert_eq!(body["author_cascade"]["winners"][0]["property"], "display");
    assert_eq!(
        body["author_cascade"]["winners"][0]["specificity"],
        serde_json::json!([1, 0, 0, 0])
    );
    assert!(body.get("semantic_tree").is_none());
}

fn stylesheet_payload(
    stylesheet_path: &str,
    selector: &str,
    property: &str,
    value: &str,
    important: bool,
) -> Value {
    let mut payload = raw_snapshot_payload();
    payload["semantic_tree"]["children"][0]["styleTrace"]["declarations"] = serde_json::json!([{
        "source_kind": "same_origin_stylesheet",
        "stylesheet_path": stylesheet_path,
        "selector": selector,
        "property": property,
        "value": value,
        "important": important
    }]);
    payload
}

#[tokio::test]
async fn fresh_style_trace_upgrades_unique_project_css_to_exact_source_coordinate() {
    let project = TempProject::new();
    project.write(
        "src/button.css",
        "/* lead */\r\n.save {\r\n  color: red;\r\n}\r\n",
    );
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("src/button.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["declarations"][0]["source_authority"]["level"],
        "exact_declaration_position"
    );
    assert_eq!(
        body["declarations"][0]["source_authority"]["file"],
        "src/button.css"
    );
    assert_eq!(body["declarations"][0]["source_authority"]["line"], 3);
    assert_eq!(body["declarations"][0]["source_authority"]["column"], 2);
    assert_eq!(
        body["declarations"][0]["source_authority"]["mapping"],
        "direct_css"
    );
    assert_eq!(
        body["author_cascade"]["winners"][0]["source_authority"]["level"],
        "runtime_inline"
    );
    let serialized = serde_json::to_string(&body).expect("serialize trace");
    assert!(!serialized.contains(&project.root.to_string_lossy().to_string()));
}

#[tokio::test]
async fn ambiguous_duplicate_css_declarations_stop_at_verified_project_file() {
    let project = TempProject::new();
    project.write(
        "src/button.css",
        ".save { color: red; }\n.save { color: red; }\n",
    );
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("src/button.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["level"], "project_file_verified");
    assert_eq!(authority["file"], "src/button.css");
    assert!(authority.get("line").is_none());
    assert!(authority.get("column").is_none());
}

#[tokio::test]
async fn project_owned_css_source_map_can_upgrade_to_original_source() {
    let project = TempProject::new();
    project.write("dist/app.css", ".save{color:red}");
    project.write("src/button.scss", "$tone: red;\n.save { color: $tone; }");
    project.write(
        "dist/app.css.map",
        serde_json::json!({
            "version": 3,
            "sources": ["../src/button.scss"],
            "names": [],
            "sourcesContent": ["MUST-NOT-LEAK"],
            "mappings": "MAAA"
        })
        .to_string(),
    );
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("dist/app.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["level"], "exact_declaration_position");
    assert_eq!(authority["file"], "src/button.scss");
    assert_eq!(authority["line"], 1);
    assert_eq!(authority["column"], 0);
    assert_eq!(authority["mapping"], "project_source_map");
    let serialized = serde_json::to_string(&body).expect("serialize trace");
    assert!(!serialized.contains("MUST-NOT-LEAK"));
    assert!(!serialized.contains(&project.root.to_string_lossy().to_string()));
}

#[tokio::test]
async fn source_map_nearest_preceding_segment_is_not_exact_css_proof() {
    let project = TempProject::new();
    project.write("dist/app.css", ".save{color:red}");
    project.write("src/button.scss", ".save { color: red; }");
    project.write(
        "dist/app.css.map",
        serde_json::json!({
            "version": 3,
            "sources": ["../src/button.scss"],
            "names": [],
            "mappings": "AAAA"
        })
        .to_string(),
    );
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("dist/app.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["level"], "exact_declaration_position");
    assert_eq!(authority["file"], "dist/app.css");
    assert_eq!(authority["column"], 6);
    assert_eq!(authority["mapping"], "direct_css");
}

#[tokio::test]
async fn source_map_escape_is_rejected_without_losing_direct_css_coordinate() {
    let project = TempProject::new();
    let outside = TempProject::new();
    project.write("dist/app.css", ".save{color:red}");
    outside.write("private.scss", ".save { color: red; }");
    project.write(
        "dist/app.css.map",
        serde_json::json!({
            "version": 3,
            "sources": [format!("../../{}/private.scss", outside.root.file_name().unwrap().to_string_lossy())],
            "names": [],
            "mappings": "MAAA"
        })
        .to_string(),
    );
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("dist/app.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["file"], "dist/app.css");
    assert_eq!(authority["mapping"], "direct_css");
}

#[tokio::test]
async fn oversized_css_is_verified_but_never_parsed_for_exact_position() {
    let project = TempProject::new();
    project.write("src/huge.css", vec![b'a'; 513 * 1024]);
    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("src/huge.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["declarations"][0]["source_authority"]["level"],
        "project_file_verified"
    );
}

#[tokio::test]
async fn inline_stylesheet_evidence_never_invents_filesystem_coordinates() {
    let (state, session_id) = test_state().await;
    let mut payload = raw_snapshot_payload();
    payload["semantic_tree"]["children"][0]["styleTrace"]["declarations"] = serde_json::json!([{
        "source_kind": "inline_stylesheet",
        "stylesheet_path": null,
        "selector": ".save",
        "property": "color",
        "value": "red",
        "important": false
    }]);
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(executor_state, session_id, payload).await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["level"], "runtime_inline");
    assert!(authority.get("file").is_none());
    assert!(authority.get("line").is_none());
    assert!(authority.get("column").is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn stylesheet_symlink_escape_never_becomes_project_file_authority() {
    use std::os::unix::fs::symlink;

    let project = TempProject::new();
    let outside = TempProject::new();
    outside.write("private.css", ".save { color: red; }");
    fs::create_dir_all(project.path("src")).expect("create source dir");
    symlink(outside.path("private.css"), project.path("src/button.css")).expect("create symlink");

    let (state, session_id) = test_state_with_cwd(Some(&project.root)).await;
    let executor_state = state.clone();
    let executor = tokio::spawn(async move {
        complete_next_snapshot(
            executor_state,
            session_id,
            stylesheet_payload("src/button.css", ".save", "color", "red", false),
        )
        .await;
    });

    let (status, body) = get_trace(state, session_id, "@save", true).await;
    executor.await.expect("snapshot executor");
    assert_eq!(status, StatusCode::OK);
    let authority = &body["declarations"][0]["source_authority"];
    assert_eq!(authority["level"], "stylesheet_hint");
    assert!(authority.get("file").is_none());
}
