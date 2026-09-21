#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use localview_instrumentation::{
    bootstrap_script, wave6::wave6_bootstrap_script, InstrumentationConfig,
};
use localview_protocol::SessionId;
use tauri::{path::BaseDirectory, Manager};

const AXE_VERSION: &str = "4.13.0";
const MAX_AXE_SOURCE_BYTES: usize = 2 * 1024 * 1024;

fn development_axe_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../node_modules/axe-core/axe.min.js")
}

fn load_local_axe_source(app: &tauri::AppHandle) -> Result<String, String> {
    let packaged = app
        .path()
        .resolve("wave6/axe.min.js", BaseDirectory::Resource)
        .map_err(|error| format!("wave6 axe resource path unavailable: {error}"))?;
    let source = std::fs::read_to_string(&packaged)
        .or_else(|_| std::fs::read_to_string(development_axe_path()))
        .map_err(|error| format!("bundled axe-core {AXE_VERSION} is unavailable: {error}"))?;
    validate_axe_source(&source)?;
    Ok(source)
}

fn validate_axe_source(source: &str) -> Result<(), String> {
    if source.is_empty() || source.len() > MAX_AXE_SOURCE_BYTES {
        return Err("bundled axe-core source violates the bounded resource size".into());
    }
    if !source.contains(AXE_VERSION) || !source.contains("axe") {
        return Err("bundled axe-core source does not match the pinned LocalView version".into());
    }
    Ok(())
}

/// Compose the exact initialization script used by LocalView-owned preview and
/// native workspace surfaces.
///
/// axe-core is read only from the application resource bundle (or the exact
/// development node_modules fallback); no page/runtime network loader exists.
pub fn managed_initialization_script(
    app: &tauri::AppHandle,
    session_id: SessionId,
) -> Result<String, String> {
    let axe = load_local_axe_source(app)?;
    Ok(format!(
        "{}\n{}\n{}\nwindow.__LOCALVIEW_WAVE6__?.installAxe(window.axe);\n{}",
        bootstrap_script(&InstrumentationConfig::default()),
        axe,
        wave6_bootstrap_script(),
        super::preview_bridge_script(session_id),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axe_resource_validation_is_version_pinned_and_bounded() {
        assert!(validate_axe_source("/* axe 4.13.0 */").is_ok());
        assert!(validate_axe_source("/* axe 4.12.0 */").is_err());
        assert!(validate_axe_source("").is_err());
    }

    #[test]
    fn development_path_never_points_at_remote_content() {
        let path = development_axe_path().to_string_lossy().replace('\\', "/");
        assert!(path.ends_with("node_modules/axe-core/axe.min.js"));
        assert!(!path.contains("http://"));
        assert!(!path.contains("https://"));
    }
}
