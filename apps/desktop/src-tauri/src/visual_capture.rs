#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    time::Duration,
};

use localview_artifacts::ArtifactStore;
use localview_capture::{CaptureTarget, SettleDecision, SettleReason, StableCapturePolicy};
use localview_native_capture::{
    capture_webview, CaptureRequest, CapturedFrame, NativeCaptureError, ViewportMeta,
};
use localview_protocol::{Rect, SessionId};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::{oneshot, Mutex};

use crate::{control_client, err, read_token, state_dir, workspace_surface};
use workspace_surface::{bridge_surface_label_allowed, workspace_navigation_allowed};

const VISUAL_ARTIFACT_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CAPTURE_SESSION_GATES: usize = 128;
const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;
const MAX_VISUAL_MASK_RECTS: usize = 256;
const MAX_MASKED_ELEMENTS: u64 = 4_096;
const MAX_CSS_VIEWPORT_DIMENSION: f64 = 100_000.0;

#[derive(Default)]
pub struct VisualCaptureState {
    pub(crate) artifacts: Mutex<Option<ArtifactStore>>,
    capture_gates: Mutex<BTreeMap<SessionId, Weak<Mutex<()>>>>,
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
    pub target: String,
    pub region: Option<Rect>,
}

#[derive(Debug, Clone)]
enum RequestedCaptureTarget {
    Viewport,
    Region(Rect),
}

impl RequestedCaptureTarget {
    fn name(&self) -> &'static str {
        match self {
            Self::Viewport => "viewport",
            Self::Region(_) => "region",
        }
    }

    fn region(&self) -> Option<Rect> {
        match self {
            Self::Viewport => None,
            Self::Region(rect) => Some(rect.clone()),
        }
    }

    fn evidence_suffix(&self) -> &'static str {
        match self {
            Self::Viewport => "visual",
            Self::Region(_) => "visual-region",
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    region: Option<Rect>,
}

#[derive(Debug, Deserialize)]
struct VisualEvidenceResponse {
    evidence_id: String,
    deduplicated: bool,
}

#[derive(Debug, Deserialize)]
struct FreezeVisualStateReceipt {
    token: String,
    paused_animations: u64,
    web_animations_supported: bool,
    viewport_css_width: f64,
    viewport_css_height: f64,
    masked_elements: u64,
    mask_rects: Vec<Rect>,
    lease_ms: u64,
}

#[tauri::command]
pub async fn capture_viewport(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String> {
    capture_target(
        app,
        state,
        session_id,
        viewport,
        revision,
        RequestedCaptureTarget::Viewport,
    )
    .await
}

#[tauri::command]
pub async fn capture_region(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    region: Rect,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String> {
    capture_target(
        app,
        state,
        session_id,
        viewport,
        revision,
        RequestedCaptureTarget::Region(region),
    )
    .await
}

async fn capture_target(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
    target: RequestedCaptureTarget,
) -> Result<VisualCaptureReceipt, String> {
    validate_viewport(&viewport)?;
    if let RequestedCaptureTarget::Region(rect) = &target {
        validate_region(rect, viewport.css_width as f64, viewport.css_height as f64)?;
    }
    preflight_managed_surface(&app, session_id)?;

    let capture_gate = session_capture_gate(&state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;

    wait_for_capture_settle(session_id).await?;
    let freeze = freeze_visual_state(session_id).await?;
    let native_result = capture_managed_surface(&app, session_id, viewport, revision).await;
    let restore_result = restore_visual_state(session_id, &freeze.token).await;

    let frame = match (native_result, restore_result) {
        (Ok(frame), Ok(())) => frame,
        (Err(native_error), Ok(())) => return Err(native_error),
        (Ok(_), Err(_)) | (Err(_), Err(_)) => {
            return Err(
                "visual capture restore acknowledgement failed; pixels discarded".to_string(),
            );
        }
    };
    validate_live_target_viewport(&frame, &freeze, &target)?;
    let frame = redact_private_pixels(frame, &freeze)?;
    let frame = apply_capture_target(frame, &freeze, &target)?;

    persist_and_register(&state, session_id, frame, &target).await
}

async fn session_capture_gate(
    state: &VisualCaptureState,
    session_id: SessionId,
) -> Result<Arc<Mutex<()>>, String> {
    let mut gates = state.capture_gates.lock().await;
    gates.retain(|_, gate| gate.strong_count() > 0);

    if let Some(gate) = gates.get(&session_id).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    if gates.len() >= MAX_CAPTURE_SESSION_GATES {
        return Err("visual capture session gate capacity exceeded".into());
    }

    let gate = Arc::new(Mutex::new(()));
    gates.insert(session_id, Arc::downgrade(&gate));
    Ok(gate)
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

fn validate_region(rect: &Rect, css_width: f64, css_height: f64) -> Result<(), String> {
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    if !css_width.is_finite()
        || !css_height.is_finite()
        || css_width <= 0.0
        || css_height <= 0.0
        || !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.width <= 0.0
        || rect.height <= 0.0
        || right > css_width
        || bottom > css_height
    {
        return Err("visual capture region is outside the bounded CSS viewport".into());
    }
    Ok(())
}

fn validate_live_target_viewport(
    frame: &CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
    target: &RequestedCaptureTarget,
) -> Result<(), String> {
    if matches!(target, RequestedCaptureTarget::Region(_))
        && (frame.viewport.css_width as f64 != freeze.viewport_css_width
            || frame.viewport.css_height as f64 != freeze.viewport_css_height)
    {
        return Err("native visual region viewport changed during capture; pixels discarded".into());
    }
    Ok(())
}

fn preflight_managed_surface(app: &tauri::AppHandle, session_id: SessionId) -> Result<(), String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !bridge_surface_label_allowed(window.label(), session_id) {
            return Err("visual capture preview/session ownership mismatch".into());
        }
        let route_url = window.url().map_err(err)?;
        if !workspace_navigation_allowed(&route_url) {
            return Err("visual capture refuses a non-loopback managed surface".into());
        }
        return Ok(());
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
            return Ok(());
        }
    }

    Err("no LocalView-managed native surface is open for this session".into())
}

async fn wait_for_capture_settle(session_id: SessionId) -> Result<(), String> {
    let policy = StableCapturePolicy::default();
    let last_reasons = Arc::new(Mutex::new(Vec::<SettleReason>::new()));
    let reasons_for_poll = last_reasons.clone();

    let settle_transaction = async move {
        let token = read_token().await?;
        let client = control_client()?;
        loop {
            let decision = client
                .get(format!(
                    "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-settle"
                ))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(err)?
                .error_for_status()
                .map_err(err)?
                .json::<SettleDecision>()
                .await
                .map_err(err)?;

            if decision.stable {
                return Ok::<(), String>(());
            }

            *reasons_for_poll.lock().await = decision.reasons;
            tokio::time::sleep(Duration::from_millis(
                decision.retry_after_ms.clamp(25, 100),
            ))
            .await;
        }
    };

    match tokio::time::timeout(
        Duration::from_millis(policy.timeout_ms),
        settle_transaction,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let reasons = last_reasons.lock().await;
            let reason_names =
                serde_json::to_string(&*reasons).unwrap_or_else(|_| "[]".to_owned());
            Err(format!(
                "stable capture settle timed out after {} ms; last_reasons={reason_names}",
                policy.timeout_ms
            ))
        }
    }
}

async fn freeze_visual_state(session_id: SessionId) -> Result<FreezeVisualStateReceipt, String> {
    let token = read_token().await?;
    let receipt = control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-freeze"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<FreezeVisualStateReceipt>()
        .await
        .map_err(err)?;

    if !valid_freeze_receipt(&receipt) {
        return Err("invalid visual freeze acknowledgement".into());
    }
    let _ = receipt.web_animations_supported;
    Ok(receipt)
}

fn valid_freeze_receipt(receipt: &FreezeVisualStateReceipt) -> bool {
    if receipt.token.is_empty()
        || receipt.paused_animations > MAX_PAUSED_ANIMATIONS
        || receipt.lease_ms != VISUAL_FREEZE_LEASE_MS
        || receipt.masked_elements > MAX_MASKED_ELEMENTS
        || receipt.mask_rects.len() > MAX_VISUAL_MASK_RECTS
        || !receipt.viewport_css_width.is_finite()
        || !receipt.viewport_css_height.is_finite()
        || receipt.viewport_css_width <= 0.0
        || receipt.viewport_css_height <= 0.0
        || receipt.viewport_css_width > MAX_CSS_VIEWPORT_DIMENSION
        || receipt.viewport_css_height > MAX_CSS_VIEWPORT_DIMENSION
    {
        return false;
    }

    receipt.mask_rects.iter().all(|rect| {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
            && right.is_finite()
            && bottom.is_finite()
            && rect.x >= 0.0
            && rect.y >= 0.0
            && rect.width > 0.0
            && rect.height > 0.0
            && right <= receipt.viewport_css_width
            && bottom <= receipt.viewport_css_height
    })
}

fn redact_private_pixels(
    mut frame: CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
) -> Result<CapturedFrame, String> {
    if freeze.mask_rects.is_empty() {
        return Ok(frame);
    }

    let (redacted_png, applied) = localview_visual::redact_png_css_rects(
        &frame.png,
        (frame.pixel_width, frame.pixel_height),
        (freeze.viewport_css_width, freeze.viewport_css_height),
        &freeze.mask_rects,
    )
    .map_err(|_| "private visual mask redaction failed; pixels discarded".to_string())?;

    if applied != freeze.mask_rects.len() {
        return Err("private visual mask application was incomplete; pixels discarded".into());
    }
    frame.png = redacted_png;
    Ok(frame)
}

fn apply_capture_target(
    mut frame: CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
    target: &RequestedCaptureTarget,
) -> Result<CapturedFrame, String> {
    let RequestedCaptureTarget::Region(rect) = target else {
        return Ok(frame);
    };

    validate_region(
        rect,
        freeze.viewport_css_width,
        freeze.viewport_css_height,
    )?;
    let cropped = localview_visual::crop_png_css_rect(
        &frame.png,
        (frame.pixel_width, frame.pixel_height),
        (freeze.viewport_css_width, freeze.viewport_css_height),
        rect,
    )
    .map_err(|_| "native visual region crop failed; pixels discarded".to_string())?;
    let decoded = localview_visual::decode_png_rgba(&cropped)
        .map_err(|_| "native visual region crop verification failed; pixels discarded".to_string())?;

    frame.png = cropped;
    frame.pixel_width = decoded.width;
    frame.pixel_height = decoded.height;
    Ok(frame)
}

async fn restore_visual_state(session_id: SessionId, token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("visual restore token is empty".into());
    }
    let control_token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-restore"
        ))
        .bearer_auth(control_token)
        .json(&serde_json::json!({"token": token}))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
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
    target: &RequestedCaptureTarget,
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
        target: target.name(),
        region: target.region(),
    };

    let token = read_token().await?;
    let evidence = control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/evidence/{}",
            target.evidence_suffix()
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
        target: target.name().to_owned(),
        region: target.region(),
    })
}
