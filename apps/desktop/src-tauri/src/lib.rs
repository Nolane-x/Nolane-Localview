#![forbid(unsafe_code)]

mod native_executor_worker;
pub mod surface_registry;
pub mod visual_capture;
pub mod workspace_surface;

use std::path::PathBuf;

use localview_instrumentation::{bootstrap_script, InstrumentationConfig};
use localview_live_bridge::{
    ActionCancellationSignal, BridgeAction, BridgeActionResult, IngestReport, ObserverBatch,
    ObserverEvent, PrivateBridgeAction,
};
use localview_protocol::{Health, Session, SessionId};
use serde::Serialize;
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Serialize)]
struct DashboardState {
    health: Health,
    sessions: Vec<Session>,
    engine: EngineInfo,
    capabilities: Vec<&'static str>,
    workspace_surface: workspace_surface::WorkspaceSurfaceSupport,
}

#[derive(Debug, Serialize)]
struct EngineInfo {
    native: &'static str,
    tier3: &'static str,
}

#[derive(Debug, Serialize)]
struct LiveSessionState {
    observer: Vec<ObserverEvent>,
    action_results: Vec<BridgeActionResult>,
}

#[tauri::command]
async fn dashboard_state() -> Result<DashboardState, String> {
    let client = control_client()?;
    let health = client
        .get("http://127.0.0.1:45454/health")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Health>()
        .await
        .map_err(err)?;
    let token = read_token().await?;
    let sessions = client
        .get("http://127.0.0.1:45454/v1/sessions")
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<Session>>()
        .await
        .map_err(err)?;
    Ok(DashboardState {
        health,
        sessions,
        engine: EngineInfo {
            native: native_engine(),
            tier3: "Chromium / Playwright on demand",
        },
        capabilities: vec![
            "Discovery",
            "Sessions",
            "Observation",
            "Instrumentation",
            "Live Bridge",
            "Semantic Diff",
            "Layout",
            "Visual Diff",
            "Responsive",
            "Source Map",
            "Source Graph",
            "Network",
            "Console",
            "A11y",
            "Performance",
            "Capture",
            "Flow Replay",
            "Design Grammar",
            "Diagnostics",
            "Reports",
            "Token Budget",
            "Evidence",
            "Causal Runtime",
            "Contracts",
            "State Space",
            "Counterfactual",
            "Verification",
            "MCP",
        ],
        workspace_surface: workspace_surface::workspace_surface_support(),
    })
}

#[tauri::command]
async fn live_session_state(session_id: SessionId) -> Result<LiveSessionState, String> {
    let token = read_token().await?;
    let client = control_client()?;
    let observer = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/observer/recent"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<ObserverEvent>>()
        .await
        .map_err(err)?;
    let action_results = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/results"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<BridgeActionResult>>()
        .await
        .map_err(err)?;
    Ok(LiveSessionState {
        observer,
        action_results,
    })
}

#[tauri::command]
async fn pause_runtime() -> Result<(), String> {
    post_control("/v1/runtime/pause").await
}

#[tauri::command]
async fn resume_runtime() -> Result<(), String> {
    post_control("/v1/runtime/resume").await
}

#[tauri::command]
async fn open_preview(
    app: tauri::AppHandle,
    session_id: String,
    url: String,
    title: String,
) -> Result<(), String> {
    let session = session_id.parse::<SessionId>().map_err(err)?;
    let label = preview_label(session);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(err)?;
        window.set_focus().map_err(err)?;
        return Ok(());
    }

    let parsed = url::Url::parse(&url).map_err(err)?;
    if !preview_navigation_allowed(&parsed) {
        return Err("LocalView preview refuses non-loopback top-level navigation".into());
    }

    let initialization_script = format!(
        "{}\n{}",
        bootstrap_script(&InstrumentationConfig::default()),
        preview_bridge_script(session)
    );

    WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(format!("{title} — LocalView"))
        .inner_size(1180.0, 760.0)
        .initialization_script(&initialization_script)
        .build()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn ingest_observer(batch: ObserverBatch) -> Result<IngestReport, String> {
    post_control_json("/v1/observer/ingest", &batch).await
}

#[tauri::command]
async fn request_bridge_action(action: BridgeAction) -> Result<(), String> {
    post_control_json::<_, serde_json::Value>("/v1/actions/request", &action)
        .await
        .map(|_| ())
}

#[tauri::command]
async fn request_private_bridge_action(action: PrivateBridgeAction) -> Result<(), String> {
    post_control_json::<_, serde_json::Value>("/v1/actions/private/request", &action)
        .await
        .map(|_| ())
}

#[tauri::command]
async fn cancel_bridge_action(signal: ActionCancellationSignal) -> Result<(), String> {
    post_control_json::<_, serde_json::Value>("/v1/actions/cancel", &signal)
        .await
        .map(|_| ())
}

fn preview_label(session: SessionId) -> String {
    format!("preview-{}", session.to_string().replace('-', "")[..17].to_string())
}

fn preview_navigation_allowed(url: &url::Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback())
}

fn preview_bridge_script(session: SessionId) -> String {
    format!(
        r#"
(() => {{
  const sessionId = {session:?};
  const emit = (kind, payload) => {{
    window.__TAURI__?.core?.invoke?.("ingest_observer", {{
      batch: {{ session_id: sessionId, generation: 1, events: [{{
        seq: Date.now(), captured_at: new Date().toISOString(), kind,
        reference: null, route: location.href, payload
      }}] }}
    }}).catch(() => {{}});
  }};
  addEventListener("error", (event) => emit("Console", {{level:"error", message:String(event.message || event.error || "error")}}));
  addEventListener("unhandledrejection", (event) => emit("Console", {{level:"error", message:String(event.reason || "unhandled rejection")}}));
}})();
"#
    )
}

fn control_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(err)
}

async fn post_control(path: &str) -> Result<(), String> {
    let client = control_client()?;
    let token = read_token().await?;
    client
        .post(format!("http://127.0.0.1:45454{path}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

async fn post_control_json<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(
    path: &str,
    body: &T,
) -> Result<R, String> {
    let client = control_client()?;
    let token = read_token().await?;
    client
        .post(format!("http://127.0.0.1:45454{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<R>()
        .await
        .map_err(err)
}

async fn read_token() -> Result<String, String> {
    let path = token_path()?;
    tokio::fs::read_to_string(path)
        .await
        .map(|token| token.trim().to_string())
        .map_err(err)
}

fn token_path() -> Result<PathBuf, String> {
    let root = dirs::data_local_dir().ok_or_else(|| "LocalView data directory is unavailable".to_string())?;
    Ok(root.join("LocalView").join("control.token"))
}

fn native_engine() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "WebView2 + Chromium HDC"
    }
    #[cfg(target_os = "macos")]
    {
        "WKWebView + Accessibility"
    }
    #[cfg(target_os = "linux")]
    {
        "WebKitGTK + Chromium HDC fallback"
    }
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn run() {
    let mut builder = tauri::Builder::default();
    builder = builder.manage(visual_capture::VisualCaptureState::default());
    builder
        .setup(|app| {
            let show = tauri::menu::MenuItemBuilder::with_id("show", "Show LocalView").build(app)?;
            let pause = tauri::menu::MenuItemBuilder::with_id("pause", "Pause Runtime").build(app)?;
            let resume = tauri::menu::MenuItemBuilder::with_id("resume", "Resume Runtime").build(app)?;
            let quit = tauri::menu::MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&show, &pause, &resume, &quit])
                .build()?;
            TrayIconBuilder::new()
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "pause" => {
                        tauri::async_runtime::spawn(async {
                            let _ = pause_runtime().await;
                        });
                    }
                    "resume" => {
                        tauri::async_runtime::spawn(async {
                            let _ = resume_runtime().await;
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_state,
            live_session_state,
            pause_runtime,
            resume_runtime,
            open_preview,
            ingest_observer,
            request_bridge_action,
            request_private_bridge_action,
            cancel_bridge_action,
            workspace_surface::workspace_surface_open,
            workspace_surface::workspace_surface_set_bounds,
            workspace_surface::workspace_surface_navigate,
            workspace_surface::workspace_surface_close,
            visual_capture::capture_visual_evidence,
            visual_capture::capture_changed_visual_diff,
            native_executor_worker::execute_native_visual_request,
            native_executor_worker::cancel_native_visual_request,
        ])
        .run(tauri::generate_context!())
        .expect("LocalView desktop runtime failed");
}
