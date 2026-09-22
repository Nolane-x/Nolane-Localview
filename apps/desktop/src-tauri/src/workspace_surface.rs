#![forbid(unsafe_code)]

#[path = "surface_registry.rs"]
pub mod surface_registry;
#[path = "surface_resource.rs"]
pub mod surface_resource;

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
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return false;
    }

    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(_)) | None => false,
    }
}

pub fn workspace_navigation_matches_origin(expected: &url::Url, candidate: &url::Url) -> bool {
    workspace_navigation_allowed(expected)
        && workspace_navigation_allowed(candidate)
        && expected.origin().ascii_serialization() == candidate.origin().ascii_serialization()
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
pub(crate) async fn workspace_surface_open(
    app: tauri::AppHandle,
    registry: tauri::State<'_, surface_registry::DesktopSurfaceRegistry>,
    bridge_authority: tauri::State<'_, super::PreviewBridgeAuthority>,
    session_id: SessionId,
    url: String,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    let bounds = validate_workspace_bounds(bounds)?;
    let parsed = parse_workspace_url(&url)?;

    #[cfg(feature = "native-workspace")]
    {
        open_native(
            &app,
            registry.inner(),
            bridge_authority.inner(),
            session_id,
            parsed,
            bounds,
        )
        .await
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, registry, bridge_authority, session_id, parsed, bounds);
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
pub(crate) async fn workspace_surface_close(
    app: tauri::AppHandle,
    registry: tauri::State<'_, surface_registry::DesktopSurfaceRegistry>,
    bridge_authority: tauri::State<'_, super::PreviewBridgeAuthority>,
    session_id: SessionId,
) -> Result<(), String> {
    #[cfg(feature = "native-workspace")]
    {
        close_native(
            &app,
            registry.inner(),
            bridge_authority.inner(),
            session_id,
        )
        .await
    }

    #[cfg(not(feature = "native-workspace"))]
    {
        let _ = (app, registry, bridge_authority, session_id);
        Err(unsupported())
    }
}

#[cfg(feature = "native-workspace")]
async fn open_native(
    app: &tauri::AppHandle,
    registry: &surface_registry::DesktopSurfaceRegistry,
    bridge_authority: &super::PreviewBridgeAuthority,
    session_id: SessionId,
    url: url::Url,
    bounds: WorkspaceBounds,
) -> Result<(), String> {
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize, Manager, WebviewUrl};

    let label = workspace_label(session_id);
    if let Some(webview) = app.get_webview(&label) {
        let current = registry
            .current(
                session_id,
                surface_registry::DesktopSurfaceKind::WorkspaceChild,
                &label,
            )
            .ok_or_else(|| {
                "native workspace platform child exists without desktop owner truth".to_string()
            })?;
        if !bridge_authority.contains_identity(&current.identity) {
            return Err("native workspace platform child exists without bridge authority".into());
        }
        webview
            .set_position(LogicalPosition::new(bounds.x, bounds.y))
            .map_err(|error| error.to_string())?;
        webview
            .set_size(LogicalSize::new(bounds.width, bounds.height))
            .map_err(|error| error.to_string())?;
        let current_url = webview.url().map_err(|error| error.to_string())?;
        if !workspace_navigation_matches_origin(&current_url, &url) {
            return Err("LocalView workspace refuses cross-origin loopback navigation".into());
        }
        webview.navigate(url).map_err(|error| error.to_string())?;
        webview.show().map_err(|error| error.to_string())?;
        let visibility_update = registry.set_visibility(
            &current.identity,
            surface_registry::DesktopSurfaceVisibility::Visible,
        );
        visibility_update.map_err(registry_error)?;
        if let Err(error) = surface_resource::update_surface_visibility(
            &current.identity,
            surface_registry::DesktopSurfaceVisibility::Visible,
        )
        .await
        {
            let _ = registry.set_visibility(&current.identity, current.visibility);
            if current.visibility == surface_registry::DesktopSurfaceVisibility::Hidden {
                let _ = webview.hide();
            }
            return Err(error);
        }
        return Ok(());
    }

    let parent = app
        .get_window("main")
        .ok_or_else(|| "LocalView main window is unavailable".to_string())?;
    let identity = registry.next_identity(
        session_id,
        surface_registry::DesktopSurfaceKind::WorkspaceChild,
        label.clone(),
    );
    let reservation = surface_resource::reserve_surface(session_id).await?;
    let attestation = bridge_authority.issue(&identity);
    let initialization_script = match super::managed_surface_initialization_script(
        app,
        session_id,
        &attestation,
    ) {
        Ok(script) => script,
        Err(error) => {
            bridge_authority.revoke(&identity);
            let _ = surface_resource::cancel_surface_reservation(&reservation).await;
            return Err(error);
        }
    };
    let expected_navigation_url = url.clone();
    let builder = WebviewBuilder::new(label, WebviewUrl::External(url))
        .initialization_script(initialization_script)
        .on_navigation(move |candidate| {
            workspace_navigation_matches_origin(&expected_navigation_url, candidate)
        });

    let webview = match parent.add_child(
        builder,
        LogicalPosition::new(bounds.x, bounds.y),
        LogicalSize::new(bounds.width, bounds.height),
    ) {
        Ok(webview) => webview,
        Err(error) => {
            let create_error = error.to_string();
            bridge_authority.revoke(&identity);
            if let Err(cancel_error) =
                surface_resource::cancel_surface_reservation(&reservation).await
            {
                return Err(format!(
                    "{create_error}; failed to cancel native surface reservation: {cancel_error}"
                ));
            }
            return Err(create_error);
        }
    };

    if let Err(error) = registry.record_created(
        identity.clone(),
        surface_registry::DesktopSurfaceVisibility::Visible,
    ) {
        bridge_authority.revoke(&identity);
        if let Err(close_error) = webview.close() {
            return Err(format!(
                "{}; failed to close native workspace surface after owner-record failure: {close_error}",
                registry_error(error)
            ));
        }
        let _ = surface_resource::cancel_surface_reservation(&reservation).await;
        return Err(registry_error(error));
    }

    if let Err(error) = surface_resource::activate_surface(
        &reservation,
        &identity,
        surface_registry::DesktopSurfaceVisibility::Visible,
    )
    .await
    {
        bridge_authority.revoke(&identity);
        if let Err(close_error) = webview.close() {
            return Err(format!(
                "{error}; failed to close native workspace surface after activation failure: {close_error}"
            ));
        }
        let _ = registry.record_closed(&identity);
        let _ = surface_resource::cancel_surface_reservation(&reservation).await;
        let _ = surface_resource::release_surface(&identity).await;
        return Err(error);
    }

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

    let webview = app
        .get_webview(&workspace_label(session_id))
        .ok_or_else(|| "native workspace surface is not open".to_string())?;
    let current = webview.url().map_err(|error| error.to_string())?;
    if !workspace_navigation_matches_origin(&current, &url) {
        return Err("LocalView workspace refuses cross-origin loopback navigation".into());
    }
    webview.navigate(url).map_err(|error| error.to_string())
}

#[cfg(feature = "native-workspace")]
async fn close_native(
    app: &tauri::AppHandle,
    registry: &surface_registry::DesktopSurfaceRegistry,
    bridge_authority: &super::PreviewBridgeAuthority,
    session_id: SessionId,
) -> Result<(), String> {
    use tauri::Manager;

    let label = workspace_label(session_id);
    let current = registry.current(
        session_id,
        surface_registry::DesktopSurfaceKind::WorkspaceChild,
        &label,
    );
    let webview = app.get_webview(&label);

    if current.is_none() {
        if webview.is_some() {
            return Err("native workspace platform child exists without desktop owner truth".into());
        }
        return Ok(());
    }
    let current = current.expect("checked above");

    if let Some(webview) = webview {
        webview.close().map_err(|error| error.to_string())?;
    }
    let owner_close = registry.record_closed(&current.identity);
    owner_close.map_err(registry_error)?;
    bridge_authority.revoke(&current.identity);
    surface_resource::release_surface(&current.identity).await
}

#[cfg(feature = "native-workspace")]
fn registry_error(error: surface_registry::DesktopSurfaceRegistryError) -> String {
    format!("desktop surface owner registry rejected lifecycle transition: {error:?}")
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
            "http://127.0.0.1:3000/app",
        ] {
            assert!(workspace_navigation_allowed(&url::Url::parse(allowed).unwrap()));
        }
        for rejected in [
            "https://localhost:5173/app",
            "https://127.0.0.1:3000/",
            "http://[::1]:8080/",
            "http://127.0.0.2:3000/",
            "http://user:secret@127.0.0.1:3000/",
            "https://example.com/",
            "file:///tmp/index.html",
            "tauri://localhost/",
            "http://localhost.example.com/",
        ] {
            assert!(!workspace_navigation_allowed(&url::Url::parse(rejected).unwrap()));
        }
    }

    #[test]
    fn managed_navigation_is_bound_to_one_exact_loopback_origin() {
        let expected = url::Url::parse("http://127.0.0.1:5173/app").unwrap();
        for allowed in [
            "http://127.0.0.1:5173/",
            "http://127.0.0.1:5173/other?query=ok#fragment",
        ] {
            assert!(workspace_navigation_matches_origin(
                &expected,
                &url::Url::parse(allowed).unwrap()
            ));
        }
        for rejected in [
            "http://127.0.0.1:5174/",
            "http://localhost:5173/",
            "https://127.0.0.1:5173/",
            "http://127.0.0.2:5173/",
        ] {
            assert!(
                !workspace_navigation_matches_origin(
                    &expected,
                    &url::Url::parse(rejected).unwrap()
                ),
                "{rejected}"
            );
        }
    }

    #[test]
    fn preview_label_stays_compatible_with_existing_bridge_windows() {
        let id = session("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(preview_surface_label(id), "preview-550e8400e29b41d4a7");
    }
}
