#![forbid(unsafe_code)]

use std::path::PathBuf;
use localview_protocol::{Health, Session};
use serde::Serialize;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;

#[derive(Debug, Serialize)]
struct DashboardState {
    health: Health,
    sessions: Vec<Session>,
    engine: EngineInfo,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct EngineInfo { native: &'static str, tier3: &'static str }

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
        engine: EngineInfo { native: native_engine(), tier3: "Chromium / Playwright on demand" },
        capabilities: vec!["Discovery", "Sessions", "Observation", "Semantic Diff", "Layout", "Visual Diff", "Responsive", "Source Map", "Network", "Console", "A11y", "Performance", "Capture", "Flow Replay", "Design Grammar", "Token Budget", "MCP"],
    })
}

#[tauri::command]
async fn pause_runtime() -> Result<(), String> { post_control("/v1/runtime/pause").await }

#[tauri::command]
async fn resume_runtime() -> Result<(), String> { post_control("/v1/runtime/resume").await }

// WebView construction remains async because creating a WebView synchronously from a Tauri command can deadlock on Windows.
#[tauri::command]
async fn open_preview(app: tauri::AppHandle, session_id: String, url: String, title: String) -> Result<(), String> {
    let label = format!("preview-{}", session_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(18).collect::<String>());
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(err)?;
        window.set_focus().map_err(err)?;
        return Ok(());
    }
    let parsed = url::Url::parse(&url).map_err(err)?;
    if !matches!(parsed.host_str(), Some("localhost") | Some("127.0.0.1") | Some("::1")) {
        return Err("LocalView preview refuses non-loopback top-level navigation".into());
    }
    WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(format!("{title} — LocalView"))
        .inner_size(1280.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .build()
        .map_err(err)?;
    Ok(())
}

async fn post_control(path: &str) -> Result<(), String> {
    let token = read_token().await?;
    control_client()?
        .post(format!("http://127.0.0.1:45454{path}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

fn control_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().map_err(err)
}

async fn read_token() -> Result<String, String> {
    tokio::fs::read_to_string(state_dir()?.join("control.token"))
        .await
        .map(|s| s.trim().to_owned())
        .map_err(err)
}

fn state_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir().map(|p| p.join("LocalView")).ok_or_else(|| "no local data directory".into())
}

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

fn native_engine() -> &'static str {
    #[cfg(target_os = "windows")]
    { "WebView2 via Tauri/WRY" }
    #[cfg(target_os = "macos")]
    { "WKWebView via Tauri/WRY" }
    #[cfg(target_os = "linux")]
    { "WebKitGTK via Tauri/WRY" }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    { "Tauri/WRY" }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let menu = MenuBuilder::new(app)
                .text("show", "Open LocalView")
                .separator()
                .text("quit", "Quit LocalView")
                .build()?;
            TrayIconBuilder::new()
                .tooltip("LocalView — AI-native localhost runtime")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" { app.exit(0); }
                    if event.id() == "show" {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
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
        .invoke_handler(tauri::generate_handler![dashboard_state, pause_runtime, resume_runtime, open_preview])
        .run(tauri::generate_context!())
        .expect("error while running LocalView desktop");
}
