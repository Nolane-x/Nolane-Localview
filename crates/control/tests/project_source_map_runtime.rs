use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
};
use chrono::Utc;
use localview_control::{router, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
};
use localview_sessions::SessionManager;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("localview-source-map-{}", Uuid::new_v4()));
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

    fn map_json(&self, source: &str, mappings: &str) -> String {
        json!({
            "version": 3,
            "sources": [source],
            "names": ["render"],
            "sourcesContent": ["MUST-NOT-LEAK-SOURCE-CONTENT"],
            "mappings": mappings
        })
        .to_string()
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn discovered(cwd: Option<&Path>) -> DiscoveredServer {
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
            title: Some("Source Map Runtime Test".into()),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state(cwd: Option<&Path>) -> (ControlState, Uuid) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions.reconcile(vec![discovered(cwd)], Utc::now()).await;
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
        .uri(format!("/v1/sessions/{session_id}/source-map/resolve"))
        .header(header::CONTENT_TYPE, "application/json");
    if authorized {
        builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
    }

    let response = router(state)
        .oneshot(
            builder
                .body(Body::from(
                    serde_json::to_vec(&body).expect("serialize request body"),
                ))
                .expect("build source-map request"),
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

fn request(generated_file: &str, line: u32, column: u32) -> Value {
    json!({
        "generated_file": generated_file,
        "generated_line": line,
        "generated_column": column
    })
}

fn assert_error(value: &Value, expected: &str) {
    assert_eq!(value.get("error").and_then(Value::as_str), Some(expected));
}

#[tokio::test]
async fn resolves_project_owned_sibling_map_without_leaking_absolute_paths() {
    let project = TempProject::new();
    project.write("dist/app.js", "console.log('generated');");
    project.write("src/App.tsx", "export const App = () => null;");
    project.write(
        "dist/app.js.map",
        project.map_json("../src/App.tsx", "AAAAA"),
    );
    let (state, session_id) = test_state(Some(&project.root)).await;

    let (status, value) = post(state, session_id, true, request("dist/app.js", 1, 0)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["generated_file"], "dist/app.js");
    assert_eq!(value["map_file"], "dist/app.js.map");
    assert_eq!(value["generated_line"], 1);
    assert_eq!(value["generated_column"], 0);
    assert_eq!(value["source"]["file"], "src/App.tsx");
    assert_eq!(value["source"]["line"], 1);
    assert_eq!(value["source"]["column"], 0);
    assert_eq!(value["source"]["name"], "render");

    let serialized = serde_json::to_string(&value).expect("serialize response");
    assert!(!serialized.contains("MUST-NOT-LEAK-SOURCE-CONTENT"));
    assert!(!serialized.contains(&project.root.to_string_lossy().to_string()));
}

#[tokio::test]
async fn requires_auth_known_session_and_backend_owned_project_root() {
    let project = TempProject::new();
    project.write("dist/app.js", "generated");
    project.write("src/App.tsx", "source");
    project.write("dist/app.js.map", project.map_json("../src/App.tsx", "AAAA"));

    let (state, session_id) = test_state(Some(&project.root)).await;
    let (status, value) = post(
        state.clone(),
        session_id,
        false,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_error(&value, "unauthorized");

    let (status, value) = post(
        state,
        Uuid::new_v4(),
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&value, "session_not_found");

    let (state, session_id) = test_state(None).await;
    let (status, value) = post(state, session_id, true, request("dist/app.js", 1, 0)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_error(&value, "project_root_unavailable");
}

#[tokio::test]
async fn rejects_caller_traversal_and_generated_position_overflow() {
    let project = TempProject::new();
    let (state, session_id) = test_state(Some(&project.root)).await;

    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("../outside.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "invalid_generated_file");

    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 0, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "generated_position_out_of_range");

    let (status, value) = post(
        state,
        session_id,
        true,
        request("dist/app.js", 1, 10_000_001),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "generated_position_out_of_range");
}

#[tokio::test]
async fn rejects_missing_oversized_invalid_and_unmapped_maps() {
    let project = TempProject::new();
    project.write("dist/app.js", "generated");
    project.write("src/App.tsx", "source");
    let (state, session_id) = test_state(Some(&project.root)).await;

    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&value, "source_map_unavailable");

    project.write("dist/app.js.map", vec![b'a'; 2 * 1024 * 1024 + 1]);
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "source_map_too_large");

    project.write("dist/app.js.map", "{not-json");
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "invalid_source_map");

    project.write("dist/app.js.map", project.map_json("../src/App.tsx", "A"));
    let (status, value) = post(state, session_id, true, request("dist/app.js", 1, 0)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "unmapped_generated_position");
}

#[tokio::test]
async fn accepts_contained_file_url_and_rejects_remote_protocol_relative_and_missing_sources() {
    let project = TempProject::new();
    project.write("dist/app.js", "generated");
    project.write("src/App.tsx", "source");
    let (state, session_id) = test_state(Some(&project.root)).await;

    let file_url = url::Url::from_file_path(project.path("src/App.tsx"))
        .expect("project source file URL")
        .to_string();
    project.write(
        "dist/app.js.map",
        project.map_json(&file_url, "AAAA"),
    );
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["source"]["file"], "src/App.tsx");

    project.write(
        "dist/app.js.map",
        project.map_json("https://evil.example/App.tsx", "AAAA"),
    );
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "source_reference_unsupported");

    project.write(
        "dist/app.js.map",
        project.map_json("//evil.example/App.tsx", "AAAA"),
    );
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_error(&value, "source_reference_unsupported");

    project.write(
        "dist/app.js.map",
        project.map_json("../src/Missing.tsx", "AAAA"),
    );
    let (status, value) = post(
        state.clone(),
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_error(&value, "source_unavailable");

    let outside = project
        .root
        .parent()
        .expect("temporary directory parent")
        .join(format!("localview-outside-{}.tsx", Uuid::new_v4()));
    fs::write(&outside, "outside").expect("write outside source");
    let outside_name = outside
        .file_name()
        .expect("outside file name")
        .to_string_lossy()
        .into_owned();
    project.write(
        "dist/app.js.map",
        project.map_json(&format!("../../{outside_name}"), "AAAA"),
    );
    let (status, value) = post(state, session_id, true, request("dist/app.js", 1, 0)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_error(&value, "source_outside_project");
    let _ = fs::remove_file(outside);
}

#[cfg(unix)]
#[tokio::test]
async fn canonicalization_blocks_generated_map_and_source_symlink_escape() {
    use std::os::unix::fs::symlink;

    let project = TempProject::new();
    let outside_dir = std::env::temp_dir().join(format!("localview-source-outside-{}", Uuid::new_v4()));
    fs::create_dir_all(&outside_dir).expect("create outside directory");
    let outside_js = outside_dir.join("outside.js");
    let outside_map = outside_dir.join("outside.js.map");
    let outside_source = outside_dir.join("Outside.tsx");
    fs::write(&outside_js, "outside generated").expect("write outside generated");
    fs::write(
        &outside_map,
        json!({
            "version": 3,
            "sources": ["Outside.tsx"],
            "names": [],
            "mappings": "AAAA"
        })
        .to_string(),
    )
    .expect("write outside map");
    fs::write(&outside_source, "outside source").expect("write outside source");

    fs::create_dir_all(project.path("dist")).expect("create dist");
    symlink(&outside_js, project.path("dist/app.js")).expect("symlink generated");
    let (state, session_id) = test_state(Some(&project.root)).await;
    let (status, value) = post(
        state,
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_error(&value, "source_outside_project");
    fs::remove_file(project.path("dist/app.js")).expect("remove generated symlink");

    project.write("dist/app.js", "generated");
    symlink(&outside_map, project.path("dist/app.js.map")).expect("symlink map");
    let (state, session_id) = test_state(Some(&project.root)).await;
    let (status, value) = post(
        state,
        session_id,
        true,
        request("dist/app.js", 1, 0),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_error(&value, "source_outside_project");
    fs::remove_file(project.path("dist/app.js.map")).expect("remove map symlink");

    project.write(
        "dist/app.js.map",
        project.map_json("../src/App.tsx", "AAAA"),
    );
    fs::create_dir_all(project.path("src")).expect("create src");
    symlink(&outside_source, project.path("src/App.tsx")).expect("symlink source");
    let (state, session_id) = test_state(Some(&project.root)).await;
    let (status, value) = post(state, session_id, true, request("dist/app.js", 1, 0)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_error(&value, "source_outside_project");

    let _ = fs::remove_dir_all(outside_dir);
}
