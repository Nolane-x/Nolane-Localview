#![forbid(unsafe_code)]

use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};

const MAX_SURFACE_EXTENT: f64 = 100_000.0;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct WorkspaceBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct WorkspaceSurfaceSupport {
    pub compiled: bool,
    pub default_mode: &'static str,
    pub reason: &'static str,
}

pub fn workspace_surface_support() -> WorkspaceSurfaceSupport {
    #[cfg(feature = "native-workspace")]
    {
        WorkspaceSurfaceSupport {
            compiled: true,
            // Intentionally conservative. The React abstraction may request native mode only
            // after overlay/chrome composition has passed the platform policy gate.
            default_mode: "iframe",
            reason: "native child WebView support is compiled; iframe remains default until overlay and platform policy are validated",
        }
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        WorkspaceSurfaceSupport {
            compiled: false,
            default_mode: "iframe",
            reason: "this LocalView desktop build does not include the native-workspace feature",
        }
    }
}

pub fn preview_surface_label(session_id: SessionId) -> String {
    surface_label("preview", session_id, 18)
}

pub fn workspace_label(session_id: SessionId) -> String {
    // Keep enough UUID material to make collisions practically irrelevant while retaining
    // a compact Tauri label and a recognizable session prefix for diagnostics.
    surface_label("workspace", session_id, 32)
}

pub fn bridge_surface_label_allowed(label: &str, session_id: SessionId) -> bool {
    label == preview_surface_label(session_id) || label == workspace_label(session_id)
}

pub fn validate_workspace_bounds(bounds: WorkspaceBounds) -> Result<WorkspaceBounds, String> {
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
    {
        return Err("workspace surface bounds must be finite".into());
    }
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Err("workspace surface width and height must be positive".into());
    }
    if bounds.width > MAX_SURFACE_EXTENT || bounds.height > MAX_SURFACE_EXTENT {
        return Err("workspace surface bounds exceed the safety limit".into());
    }
    if bounds.x.abs() > MAX_SURFACE_EXTENT || bounds.y.abs() > MAX_SURFACE_EXTENT {
        return Err("workspace surface position exceeds the safety limit".into());
    }
    Ok(bounds)
}

pub fn workspace_navigation_allowed(url: &url::Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    ) && matches!(url.scheme(), "http" | "https")
}

fn surface_label(prefix: &str, session_id: SessionId, max_id_chars: usize) -> String {
    let id = session_id
        .to_string()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(max_id_chars)
        .collect::<String>();
    format!("{prefix}-{id}")
}

fn parse_workspace_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|error| error.to_string())?;
    if !workspace_navigation_allowed(&parsed) {
        return Err("LocalView workspace refuses non-loopback top-level navigation".into());
    }
    Ok(parsed)
}

fn unsupported() -> String {
    "native workspace surfaces are not compiled into this LocalView build".into()
}

#[tauri::command]
pub async fn workspace_surface_open(
    app: tauri::AppHandle,
    session_id: SessionId,
    url: String,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    let bounds = validate_workspace_bounds(bounds)?;
    let parsed = parse_workspace_url(&url)?;

    #[cfg(feature = "native-workspace")]
    {
        open_native(&app, session_id, parsed, bounds)
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, session_id, parsed, bounds);
        Err(unsupported())
    }
}

#[tauri::command]
pub async fn workspace_surface_set_bounds(
    app: tauri::AppHandle,
    session_id: SessionId,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    let bounds = validate_workspace_bounds(bounds)?;

    #[cfg(feature = "native-workspace")]
    {
        set_native_bounds(&app, session_id, bounds)
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, session_id, bounds);
        Err(unsupported())
    }
}

#[tauri::command]
pub async fn workspace_surface_navigate(
    app: tauri::AppHandle,
    session_id: SessionId,
    url: String,
) -> Result<(), String> {
    let parsed = parse_workspace_url(&url)?;

    #[cfg(feature = "native-workspace")]
    {
        navigate_native(&app, session_id, parsed)
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, session_id, parsed);
        Err(unsupported())
    }
}

#[tauri::command]
pub async fn workspace_surface_close(
    app: tauri::AppHandle,
    session_id: SessionId,
) -> Result<(), String> {
    #[cfg(feature = "native-workspace")]
    {
        close_native(&app, session_id)
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, session_id);
        Err(unsupported())
    }
}

#[cfg(feature = "native-workspace")]
fn open_native(
    app: &tauri::AppHandle,
    session_id: SessionId,
    url: url::Url,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    use localview_instrumentation::{bootstrap_script, InstrumentationConfig};
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};

    let label = workspace_label(session_id);
    if let Some(webview) = app.get_webview(&label) {
        webview
            .set_position(LogicalPosition::new(bounds.x, bounds.y))
            .map_err(|error| error.to_string())?;
        webview
            .set_size(LogicalSize::new(bounds.width, bounds.height))
            .map_err(|error| error.to_string())?;
        webview.navigate(url).map_err(|error| error.to_string())?;
        webview.show().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let parent = app
        .get_window("main")
        .ok_or_else(|| "LocalView main window is unavailable".to_string())?;
    let initialization_script = format!(
        "{}\n{}",
        bootstrap_script(&InstrumentationConfig::default()),
        super::preview_bridge_script(session_id)
    );
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .initialization_script(initialization_script)
        .on_navigation(workspace_navigation_allowed);

    parent
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width, bounds.height),
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "native-workspace")]
fn set_native_bounds(
    app: &tauri::AppHandle,
    session_id: SessionId,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    use tauri::{LogicalPosition, LogicalSize, Manager};

    let webview = app
        .get_webview(&workspace_label(session_id))
        .ok_or_else(|| "native workspace surface is not open".to_string())?;
    webview
        .set_position(LogicalPosition::new(bounds.x, bounds.y))
        .map_err(|error| error.to_string())?;
    webview
        .set_size(LogicalSize::new(bounds.width, bounds.height))
        .map_err(|error| error.to_string())
}

#[cfg(feature = "native-workspace")]
fn navigate_native(
    app: &tauri::AppHandle,
    session_id: SessionId,
    url: url::Url,
) -> Result<(), String> {
    use tauri::Manager;

    app.get_webview(&workspace_label(session_id))
        .ok_or_else(|| "native workspace surface is not open".to_string())?
        .navigate(url)
        .map_err(|error| error.to_string())
}

#[cfg(feature = "native-workspace")]
fn close_native(app: &tauri::AppHandle, session_id: SessionId) -> Result<(), String> {
    use tauri::Manager;

    if let Some(webview) = app.get_webview(&workspace_label(session_id)) {
        webview.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(value: &str) -> SessionId {
        value.parse().expect("valid UUID")
    }

    #[test]
    fn loopback_navigation_rejects_external_hosts_and_non_http_schemes() {
        for allowed in [
            "http://localhost:5173/",
            "https://localhost:5173/app",
            "http://127.0.0.1:3000/",
            "http://[::1]:8080/",
        ] {
            assert!(workspace_navigation_allowed(&url::Url::parse(allowed).unwrap()));
        }
        for rejected in [
            "https://example.com/",
            "file:///tmp/index.html",
            "tauri://localhost/",
            "http://localhost.example.com/",
        ] {
            assert!(!workspace_navigation_allowed(&url::Url::parse(rejected).unwrap()));
        }
    }

    #[test]
    fn preview_label_stays_compatible_with_existing_bridge_windows() {
        let id = session("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(preview_surface_label(id), "preview-550e8400e29b41d4a7");
    }
}
