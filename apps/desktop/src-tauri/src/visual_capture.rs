#![forbid(unsafe_code)]

use std::time::Duration;

use localview_artifacts::ArtifactStore;
use localview_capture::CaptureTarget;
use localview_native_capture::{
    capture_webview, CapturedFrame, CaptureRequest, NativeCaptureError, ViewportMeta,
};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::{oneshot, Mutex};

use crate::{control_client, err, read_token, state_dir, workspace_surface};
use workspace_surface::{bridge_surface_label_allowed, workspace_navigation_allowed};

const VISUAL_ARTIFACT_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Default)]
pub struct VisualCaptureState {
    pub(crate) artifacts: Mutex<Option<ArtifactStore>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisualCaptureReceipt {
    pub artifact_id: String,
    pub evidence_id: String,
    pub deduplicated: bool,
    pub backend: String,
    pub route: String,
    pub viewport: ViewportMeta,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub revision: Option<String>,
    pub captured_at_unix_ms: u64,
}

#[derive(Debug, Serialize)]
struct VisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    target: &'static str,
}

#[derive(Debug, Deserialize)]
struct VisualEvidenceResponse {
    evidence_id: String,
    deduplicated: bool,
}

#[tauri::command]
pub async fn capture_viewport(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String> {
    validate_viewport(&viewport)?;

    let frame = capture_managed_surface(&app, session_id, viewport, revision).await?;
    persist_and_register(&state, session_id, frame).await
}

fn validate_viewport(viewport: &ViewportMeta) -> Result<(), String> {
    if viewport.css_width == 0 || viewport.css_height == 0 {
        return Err("visual capture viewport dimensions must be positive".into());
    }
    if !viewport.device_scale_factor.is_finite()
        || viewport.device_scale_factor <= 0.0
        || viewport.device_scale_factor > 8.0
    {
        return Err("visual capture device scale factor is outside the safety range".into());
    }
    Ok(())
}

async fn capture_managed_surface(
    app: &tauri::AppHandle,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<CapturedFrame, String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !bridge_surface_label_allowed(window.label(), session_id) {
            return Err("visual capture preview/session ownership mismatch".into());
        }
        let route_url = window.url().map_err(err)?;
        if !workspace_navigation_allowed(&route_url) {
            return Err("visual capture refuses a non-loopback managed surface".into());
        }
        let request = CaptureRequest {
            target: CaptureTarget::Viewport,
            viewport,
            route: route_url.to_string(),
            revision,
        };
        let (tx, rx) = oneshot::channel();
        window
            .with_webview(move |platform| {
                capture_webview(platform, request, move |result| {
                    let _ = tx.send(result);
                });
            })
            .map_err(err)?;
        return await_capture(rx).await;
    }

    #[cfg(feature = "native-workspace")]
    {
        let workspace_label = workspace_surface::workspace_label(session_id);
        if let Some(window) = app.get_webview(&workspace_label) {
            if !bridge_surface_label_allowed(window.label(), session_id) {
                return Err("visual capture workspace/session ownership mismatch".into());
            }
            let route_url = window.url().map_err(err)?;
            if !workspace_navigation_allowed(&route_url) {
                return Err("visual capture refuses a non-loopback managed surface".into());
            }
            let request = CaptureRequest {
                target: CaptureTarget::Viewport,
                viewport,
                route: route_url.to_string(),
                revision,
            };
            let (tx, rx) = oneshot::channel();
            window
                .with_webview(move |platform| {
                    capture_webview(platform, request, move |result| {
                        let _ = tx.send(result);
                    });
                })
                .map_err(err)?;
            return await_capture(rx).await;
        }
    }

    Err("no LocalView-managed native surface is open for this session".into())
}

async fn await_capture(
    receiver: oneshot::Receiver<Result<CapturedFrame, NativeCaptureError>>,
) -> Result<CapturedFrame, String> {
    tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .map_err(|_| "native visual capture timed out".to_string())?
        .map_err(|_| "native visual capture callback closed before completion".to_string())?
        .map_err(err)
}

async fn persist_and_register(
    state: &VisualCaptureState,
    session_id: SessionId,
    frame: CapturedFrame,
) -> Result<VisualCaptureReceipt, String> {
    let CapturedFrame {
        png,
        pixel_width,
        pixel_height,
        backend,
        viewport,
        route,
        revision,
        captured_at_unix_ms,
    } = frame;

    let artifact_id = {
        let mut artifacts = state.artifacts.lock().await;
        if artifacts.is_none() {
            let root = state_dir()?.join("artifacts").join("visual");
            *artifacts = Some(
                ArtifactStore::open(root, VISUAL_ARTIFACT_BUDGET_BYTES)
                    .await
                    .map_err(err)?,
            );
        }
        artifacts
            .as_mut()
            .expect("visual artifact store initialized above")
            .put("visual/png", &png)
            .await
            .map_err(err)?
            .id
    };
    drop(png);

    let backend = backend.to_string();
    let captured_at_for_api = i64::try_from(captured_at_unix_ms)
        .map_err(|_| "visual capture timestamp exceeds daemon range".to_string())?;
    let metadata = VisualEvidenceRequest {
        artifact_id: artifact_id.clone(),
        pixel_width,
        pixel_height,
        backend: backend.clone(),
        route: route.clone(),
        viewport: viewport.clone(),
        revision: revision.clone(),
        captured_at_unix_ms: captured_at_for_api,
        target: "viewport",
    };

    let token = read_token().await?;
    let evidence = control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/evidence/visual"
        ))
        .bearer_auth(token)
        .json(&metadata)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<VisualEvidenceResponse>()
        .await
        .map_err(err)?;

    Ok(VisualCaptureReceipt {
        artifact_id,
        evidence_id: evidence.evidence_id,
        deduplicated: evidence.deduplicated,
        backend,
        route,
        viewport,
        pixel_width,
        pixel_height,
        revision,
        captured_at_unix_ms,
    })
}
