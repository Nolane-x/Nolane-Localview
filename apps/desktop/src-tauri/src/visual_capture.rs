#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
    time::Duration,
};

use localview_artifacts::ArtifactStore;
use localview_capture::{CaptureTarget, SettleDecision, SettleReason, StableCapturePolicy};
use localview_native_capture::{
    capture_webview, CaptureRequest, CapturedFrame, NativeCaptureBackend, NativeCaptureError,
    ViewportMeta,
};
use localview_protocol::{ElementRef, PageSnapshot, Rect, SemanticNode, SessionId};
use localview_resource_governor::{
    RetainedResourceBudget, RetainedResourceKind, RetainedResourceLedger,
    RetainedResourceViolation,
};
use localview_responsive::{
    analyze_responsive_series, bounded_adaptive_sweep, build_responsive_contact_sheet,
    deduplicate_responsive_issues, discover_breakpoint, evaluate_responsive_observation,
    plan_canonical_sweep, resolve_observed_transition, ContactSheetPolicy, LayoutProbe,
    ObservedTransitionResolution, ResponsiveDetectorState, ResponsiveFrame, ResponsiveIssue,
    ResponsiveNodeObservation, ResponsiveObservation, ResponsivePresetId, ResponsiveProbeEvaluation,
    ResponsiveProbeSample, ResponsiveRect, ResponsiveSweepPlan, DEFAULT_ADAPTIVE_INITIAL_PROBE_CAP,
    DEFAULT_ADAPTIVE_MAX_WIDTH, DEFAULT_ADAPTIVE_MIN_WIDTH, DEFAULT_ADAPTIVE_PROBE_CAP,
    DEFAULT_BREAKPOINT_TOLERANCE_PX, MAX_RESPONSIVE_OBSERVATION_NODES,
};
use localview_visual::{
    decode_png_rgba, encode_png_rgba, plan_changed_css_regions, plan_full_page,
    project_full_page_output, stitch_full_page_tile, ChangedRegionPlan, ChangedRegionPolicy,
    FullPageError, FullPagePlan, FullPagePolicy, RgbaImage, VisualBaselineCache,
    VisualBaselineContext,
};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::{oneshot, Mutex};

use crate::{control_client, err, read_token, state_dir, workspace_surface};
use workspace_surface::{bridge_surface_label_allowed, workspace_navigation_allowed};

const VISUAL_ARTIFACT_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
const VISUAL_BASELINE_BUDGET_BYTES: usize = 96 * 1024 * 1024;
const MAX_VISUAL_BASELINES: usize = 32;
const MAX_CAPTURE_SESSION_GATES: usize = 128;
const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;
const FULL_PAGE_TRANSACTION_TIMEOUT_MS: u64 = 30_000;
const FULL_PAGE_CLEANUP_RESERVE_MS: u64 = 2_000;
const RESPONSIVE_TRANSACTION_TIMEOUT_MS: u64 = 30_000;
const RESPONSIVE_CLEANUP_RESERVE_MS: u64 = 5_000;
const RESPONSIVE_RESIZE_TIMEOUT_MS: u64 = 2_000;
const RESPONSIVE_RESIZE_POLL_MS: u64 = 25;
const RESPONSIVE_PREVIEW_MIN_WIDTH: f64 = 640.0;
const RESPONSIVE_PREVIEW_MIN_HEIGHT: f64 = 480.0;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;
const MAX_POSITIONAL_SCAN_ELEMENTS: u64 = 4_096;
const MAX_VISUAL_MASK_RECTS: usize = 256;
const MAX_MASKED_ELEMENTS: u64 = 4_096;
const MAX_CSS_VIEWPORT_DIMENSION: f64 = 100_000.0;
const TRUSTED_VIEWPORT_INTEGRAL_TOLERANCE_CSS: f64 = 0.01;

pub struct VisualCaptureState {
    pub(crate) artifacts: Mutex<Option<ArtifactStore>>,
    capture_gates: Mutex<BTreeMap<SessionId, Weak<Mutex<()>>>>,
    baselines: Mutex<Option<VisualBaselineCache>>,
    retained_resources: RetainedResourceLedger,
}

impl Default for VisualCaptureState {
    fn default() -> Self {
        let cache_bytes = u64::try_from(VISUAL_BASELINE_BUDGET_BYTES)
            .expect("visual baseline retained budget fits u64");
        let retained_resources = RetainedResourceLedger::new(RetainedResourceBudget {
            capture_storage_bytes: VISUAL_ARTIFACT_BUDGET_BYTES,
            cache_bytes,
        })
        .expect("visual retained resource budgets are non-zero");
        Self {
            artifacts: Mutex::new(None),
            capture_gates: Mutex::new(BTreeMap::new()),
            baselines: Mutex::new(None),
            retained_resources,
        }
    }
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

#[derive(Debug, Clone, Serialize)]
pub struct ChangedRegionCaptureReceipt {
    pub mode: &'static str,
    pub changed_ratio: f64,
    pub receipts: Vec<VisualCaptureReceipt>,
    pub visual_diff_evidence_id: String,
    pub visual_diff_deduplicated: bool,
    pub baseline_cached: bool,
}

#[derive(Debug)]
struct ChangedCaptureEmission {
    mode: &'static str,
    changed_ratio: f64,
    receipts: Vec<VisualCaptureReceipt>,
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

#[derive(Debug, Serialize)]
struct VisualDiffEvidenceRequest {
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    mode: &'static str,
    changed_ratio: f64,
    visual_evidence_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct VisualDiffEvidenceResponse {
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

#[derive(Debug, Clone, Serialize)]
pub struct ResponsiveSweepReceipt {
    pub artifact_id: String,
    pub evidence_id: String,
    pub deduplicated: bool,
    pub route: String,
    pub contact_sheet_pixel_width: u32,
    pub contact_sheet_pixel_height: u32,
    pub viewports: Vec<ResponsiveViewportReceipt>,
    pub adaptive: ResponsiveAdaptiveReceipt,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsiveViewportReceipt {
    pub preset: ResponsivePresetId,
    pub css_width: u32,
    pub css_height: u32,
    pub device_scale_factor: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub sheet_x: u32,
    pub sheet_y: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsiveAdaptiveReceipt {
    pub detector: String,
    pub probe_cap: usize,
    pub probes: Vec<ResponsiveAdaptiveProbeReceipt>,
    pub transition: ObservedTransitionResolution,
    pub issues: Vec<ResponsiveIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsiveAdaptiveProbeReceipt {
    pub css_width: u32,
    pub css_height: u32,
    pub snapshot_version: u64,
    pub state: ResponsiveDetectorState,
    pub issue_count: usize,
}

#[derive(Debug, Serialize)]
struct ResponsiveVisualEvidenceRequest {
    artifact_id: String,
    route: String,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    contact_sheet_pixel_width: u32,
    contact_sheet_pixel_height: u32,
    viewports: Vec<ResponsiveViewportEvidence>,
}

#[derive(Debug, Clone, Serialize)]
struct ResponsiveViewportEvidence {
    preset: ResponsivePresetId,
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
    pixel_width: u32,
    pixel_height: u32,
    sheet_x: u32,
    sheet_y: u32,
}

#[derive(Debug)]
struct ResponsiveCapturedViewport {
    responsive_frame: ResponsiveFrame,
    device_scale_factor: f64,
    route: String,
    revision: Option<String>,
    captured_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
struct ResponsivePreviewState {
    original_physical_width: u32,
    original_physical_height: u32,
    canonical_route: String,
}

#[derive(Debug)]
struct ResponsiveTransactionOutput {
    captured: Vec<ResponsiveCapturedViewport>,
    adaptive: ResponsiveAdaptiveReceipt,
}

#[derive(Debug, Clone)]
struct LiveResponsiveProbe {
    observation: ResponsiveObservation,
    evaluation: ResponsiveProbeEvaluation,
}

#[derive(Debug)]
struct LiveResponsiveProbeState {
    probes: BTreeMap<u32, LiveResponsiveProbe>,
    attempted_widths: BTreeSet<u32>,
    failure: Option<String>,
}

struct LiveResponsiveLayoutProbe<'a> {
    window: &'a tauri::WebviewWindow,
    registry: &'a workspace_surface::surface_registry::DesktopSurfaceRegistry,
    session_id: SessionId,
    preview_label: &'a str,
    expected_route: &'a str,
    css_height: u32,
    deadline: tokio::time::Instant,
    probe_cap: usize,
    state: Mutex<LiveResponsiveProbeState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FullPageCaptureReceipt {
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
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub tile_count: usize,
}

#[derive(Debug, Deserialize)]
struct FullPageFreezeVisualStateReceipt {
    token: String,
    paused_animations: u64,
    web_animations_supported: bool,
    viewport_css_width: f64,
    viewport_css_height: f64,
    masked_elements: u64,
    mask_rects: Vec<Rect>,
    scroll_x: f64,
    scroll_y: f64,
    document_css_width: f64,
    document_css_height: f64,
    lease_ms: u64,
}

#[derive(Debug, Deserialize)]
struct CaptureScrollReceipt {
    requested_y: f64,
    actual_x: f64,
    actual_y: f64,
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
}

#[derive(Debug, Deserialize)]
struct CaptureTileProbeReceipt {
    scroll_x: f64,
    scroll_y: f64,
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
    masked_elements: u64,
    mask_rects: Vec<Rect>,
    positional_elements_scanned: u64,
    visible_fixed_or_sticky: bool,
}

#[derive(Debug, Serialize)]
struct FullPageVisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    document_css_width: f64,
    document_css_height: f64,
    tile_count: usize,
    scroll_offsets_y: Vec<f64>,
}

struct FullPageStitchedFrame {
    image: RgbaImage,
    backend: String,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: u64,
}

struct FullPageTransactionOutput {
    frame: FullPageStitchedFrame,
    plan: FullPagePlan,
}

#[tauri::command]
pub async fn capture_responsive_sweep(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    registry: tauri::State<'_, workspace_surface::surface_registry::DesktopSurfaceRegistry>,
    session_id: SessionId,
    presets: Vec<ResponsivePresetId>,
) -> Result<ResponsiveSweepReceipt, String> {
    use workspace_surface::surface_registry::DesktopSurfaceKind;

    let plan = plan_canonical_sweep(&presets)
        .map_err(|_| "responsive_invalid_presets".to_string())?;
    let preview_label = workspace_surface::preview_surface_label(session_id);
    let window = app
        .get_webview_window(&preview_label)
        .ok_or_else(|| "responsive_preview_unavailable".to_string())?;
    let current = registry.current(
        session_id,
        DesktopSurfaceKind::PreviewWindow,
        &preview_label,
    )
    .ok_or_else(|| "responsive_preview_owner_mismatch".to_string())?;
    if current.identity.label != preview_label
        || current.identity.session_id != session_id
        || current.identity.owner_instance_id != registry.owner_instance_id()
        || window.label() != preview_label
        || !bridge_surface_label_allowed(window.label(), session_id)
    {
        return Err("responsive_preview_owner_mismatch".into());
    }
    if window.is_maximized().map_err(|_| "responsive_preview_unavailable".to_string())? {
        return Err("responsive_preview_maximized".into());
    }
    if window.is_fullscreen().map_err(|_| "responsive_preview_unavailable".to_string())? {
        return Err("responsive_preview_fullscreen".into());
    }
    let route = window
        .url()
        .map_err(|_| "responsive_preview_unavailable".to_string())?;
    if !workspace_navigation_allowed(&route) {
        return Err("responsive_preview_unavailable".into());
    }
    let canonical_route = canonical_visual_diff_route(route.as_str())
        .map_err(|_| "responsive_preview_unavailable".to_string())?;
    let original = window
        .inner_size()
        .map_err(|_| "responsive_preview_unavailable".to_string())?;
    if original.width == 0 || original.height == 0 {
        return Err("responsive_preview_unavailable".into());
    }
    let preview_state = ResponsivePreviewState {
        original_physical_width: original.width,
        original_physical_height: original.height,
        canonical_route: canonical_route.clone(),
    };

    let capture_gate = session_capture_gate(&state, session_id)
        .await
        .map_err(|_| "responsive_preview_unavailable".to_string())?;
    let _capture_guard = capture_gate.lock().await;
    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(RESPONSIVE_TRANSACTION_TIMEOUT_MS);
    let work_deadline = deadline - Duration::from_millis(RESPONSIVE_CLEANUP_RESERVE_MS);

    window
        .set_min_size(None::<tauri::LogicalSize<f64>>)
        .map_err(|_| "responsive_resize_failed".to_string())?;

    let work = tokio::time::timeout_at(work_deadline, async {
        let mut captured = Vec::with_capacity(plan.presets.len());
        for preset in plan.presets.iter().copied() {
            let viewport = preset.viewport();
            window
                .set_size(tauri::LogicalSize::new(
                    f64::from(viewport.width),
                    f64::from(viewport.height),
                ))
                .map_err(|_| "responsive_resize_failed".to_string())?;
            wait_for_responsive_size_convergence(
                &window,
                viewport.width,
                viewport.height,
                work_deadline,
            )
            .await?;

            let current_route = window
                .url()
                .map_err(|_| "responsive_preview_unavailable".to_string())?;
            let current_route = canonical_visual_diff_route(current_route.as_str())
                .map_err(|_| "responsive_route_drift".to_string())?;
            if current_route != canonical_route {
                return Err("responsive_route_drift".to_string());
            }

            let scale_factor = window
                .scale_factor()
                .map_err(|_| "responsive_preview_unavailable".to_string())?;
            validate_trusted_scale_factor(scale_factor)
                .map_err(|_| "responsive_viewport_mismatch".to_string())?;
            let viewport_meta = ViewportMeta {
                css_width: viewport.width,
                css_height: viewport.height,
                device_scale_factor: scale_factor,
            };

            let frame = capture_responsive_viewport_after_resize(
                &app,
                &window,
                session_id,
                preset,
                viewport_meta,
                &canonical_route,
            )
            .await?;

            let image = decode_png_rgba(&frame.png)
                .map_err(|_| "responsive_redaction_failed".to_string())?;
            if image.width != frame.pixel_width || image.height != frame.pixel_height {
                return Err("responsive_viewport_mismatch".to_string());
            }
            captured.push(ResponsiveCapturedViewport {
                responsive_frame: ResponsiveFrame {
                    preset,
                    viewport,
                    pixel_width: image.width,
                    pixel_height: image.height,
                    rgba: image.data,
                },
                device_scale_factor: scale_factor,
                route: canonical_route.clone(),
                revision: frame.revision,
                captured_at_unix_ms: frame.captured_at_unix_ms,
            });
        }
        Ok::<Vec<ResponsiveCapturedViewport>, String>(captured)
    })
    .await
    .unwrap_or_else(|_| Err("responsive_transaction_timeout".to_string()));

    let restore = restore_responsive_preview(&window, session_id, &preview_state, deadline).await;
    let captured = match (work, restore) {
        (Ok(captured), Ok(())) => captured,
        (Err(primary), Ok(())) => return Err(primary),
        (Ok(_), Err(_)) => return Err("responsive_restore_failed".into()),
        (Err(primary), Err(_)) => return Err(format!("{primary};responsive_restore_failed")),
    };

    let restored_route = window
        .url()
        .map_err(|_| "responsive_restore_failed".to_string())?;
    let restored_route = canonical_visual_diff_route(restored_route.as_str())
        .map_err(|_| "responsive_route_drift".to_string())?;
    if restored_route != preview_state.canonical_route {
        return Err("responsive_route_drift".into());
    }

    let responsive_frames = captured
        .iter()
        .map(|entry| entry.responsive_frame.clone())
        .collect::<Vec<_>>();
    let contact_sheet = build_responsive_contact_sheet(
        &plan,
        &responsive_frames,
        ContactSheetPolicy::default(),
    )
    .map_err(|error| match error {
        localview_responsive::ResponsiveError::FrameMemoryBudgetExceeded
        | localview_responsive::ResponsiveError::ContactSheetMemoryBudgetExceeded => {
            "responsive_memory_budget_exceeded".to_string()
        }
        _ => "responsive_contact_sheet_failed".to_string(),
    })?;
    let image = RgbaImage {
        width: contact_sheet.geometry.pixel_width,
        height: contact_sheet.geometry.pixel_height,
        data: contact_sheet.rgba,
    };
    image
        .validate()
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?;
    let png = encode_png_rgba(&image)
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?;

    if tokio::time::Instant::now() >= deadline {
        return Err("responsive_transaction_timeout".into());
    }
    tokio::time::timeout_at(
        deadline,
        persist_responsive_contact_sheet_and_register(
            &state,
            session_id,
            &plan,
            &captured,
            &contact_sheet.geometry,
            png,
            &preview_state.canonical_route,
        ),
    )
    .await
    .map_err(|_| "responsive_transaction_timeout".to_string())?
}

async fn wait_for_responsive_size_convergence(
    window: &tauri::WebviewWindow,
    css_width: u32,
    css_height: u32,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    let local_deadline = std::cmp::min(
        deadline,
        tokio::time::Instant::now() + Duration::from_millis(RESPONSIVE_RESIZE_TIMEOUT_MS),
    );
    loop {
        let scale = window
            .scale_factor()
            .map_err(|_| "responsive_resize_failed".to_string())?;
        validate_trusted_scale_factor(scale)
            .map_err(|_| "responsive_resize_failed".to_string())?;
        let size = window
            .inner_size()
            .map_err(|_| "responsive_resize_failed".to_string())?;
        let expected_width = (f64::from(css_width) * scale).round();
        let expected_height = (f64::from(css_height) * scale).round();
        if (f64::from(size.width) - expected_width).abs() <= 2.0
            && (f64::from(size.height) - expected_height).abs() <= 2.0
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= local_deadline {
            return Err("responsive_resize_timeout".into());
        }
        tokio::time::sleep(Duration::from_millis(RESPONSIVE_RESIZE_POLL_MS)).await;
    }
}

async fn capture_responsive_viewport_after_resize(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    session_id: SessionId,
    preset: ResponsivePresetId,
    viewport: ViewportMeta,
    expected_route: &str,
) -> Result<CapturedFrame, String> {
    wait_for_capture_settle(session_id)
        .await
        .map_err(|_| "responsive_settle_failed".to_string())?;
    let freeze = freeze_visual_state(session_id)
        .await
        .map_err(|_| "responsive_freeze_failed".to_string())?;
    if trusted_css_dimension(freeze.viewport_css_width, "width").ok() != Some(viewport.css_width)
        || trusted_css_dimension(freeze.viewport_css_height, "height").ok() != Some(viewport.css_height)
    {
        let _ = restore_visual_state(session_id, &freeze.token).await;
        return Err("responsive_viewport_mismatch".into());
    }

    let native_result = capture_managed_surface_preview_only(
        app,
        window,
        session_id,
        viewport.clone(),
        None,
    )
    .await;
    let restore_result = restore_visual_state(session_id, &freeze.token).await;
    let frame = match (native_result, restore_result) {
        (Ok(frame), Ok(())) => frame,
        (Err(_), Ok(())) => return Err("responsive_native_capture_failed".into()),
        (Ok(_), Err(_)) | (Err(_), Err(_)) => return Err("responsive_restore_failed".into()),
    };
    let frame = redact_private_pixels(frame, &freeze)
        .map_err(|_| "responsive_redaction_failed".to_string())?;

    let canonical_route = canonical_visual_diff_route(&frame.route)
        .map_err(|_| "responsive_route_drift".to_string())?;
    if canonical_route != expected_route
        || frame.viewport.css_width != preset.viewport().width
        || frame.viewport.css_height != preset.viewport().height
        || (frame.viewport.device_scale_factor - viewport.device_scale_factor).abs() > f64::EPSILON
    {
        return Err("responsive_viewport_mismatch".into());
    }
    Ok(frame)
}

async fn capture_managed_surface_preview_only(
    _app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<CapturedFrame, String> {
    if !bridge_surface_label_allowed(window.label(), session_id) {
        return Err("responsive_preview_owner_mismatch".into());
    }
    let route_url = window
        .url()
        .map_err(|_| "responsive_preview_unavailable".to_string())?;
    if !workspace_navigation_allowed(&route_url) {
        return Err("responsive_preview_unavailable".into());
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
        .map_err(|_| "responsive_native_capture_failed".to_string())?;
    await_capture(rx)
        .await
        .map_err(|_| "responsive_native_capture_failed".to_string())
}

async fn restore_responsive_preview(
    window: &tauri::WebviewWindow,
    session_id: SessionId,
    preview: &ResponsivePreviewState,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    window
        .set_size(tauri::PhysicalSize::new(
            preview.original_physical_width,
            preview.original_physical_height,
        ))
        .map_err(|_| "responsive_restore_failed".to_string())?;
    window
        .set_min_size(Some(tauri::LogicalSize::new(
            RESPONSIVE_PREVIEW_MIN_WIDTH,
            RESPONSIVE_PREVIEW_MIN_HEIGHT,
        )))
        .map_err(|_| "responsive_restore_failed".to_string())?;

    let local_deadline = std::cmp::min(
        deadline,
        tokio::time::Instant::now() + Duration::from_millis(RESPONSIVE_RESIZE_TIMEOUT_MS),
    );
    loop {
        let size = window
            .inner_size()
            .map_err(|_| "responsive_restore_failed".to_string())?;
        if size.width == preview.original_physical_width
            && size.height == preview.original_physical_height
        {
            break;
        }
        if tokio::time::Instant::now() >= local_deadline {
            return Err("responsive_restore_failed".into());
        }
        tokio::time::sleep(Duration::from_millis(RESPONSIVE_RESIZE_POLL_MS)).await;
    }

    tokio::time::timeout_at(deadline, wait_for_capture_settle(session_id))
        .await
        .map_err(|_| "responsive_restore_failed".to_string())?
        .map_err(|_| "responsive_restore_failed".to_string())
}

async fn persist_responsive_contact_sheet_and_register(
    state: &VisualCaptureState,
    session_id: SessionId,
    plan: &ResponsiveSweepPlan,
    captured: &[ResponsiveCapturedViewport],
    geometry: &localview_responsive::ContactSheetGeometry,
    png: Vec<u8>,
    route: &str,
) -> Result<ResponsiveSweepReceipt, String> {
    let artifact_id = {
        let mut artifacts = state.artifacts.lock().await;
        if artifacts.is_none() {
            let root = state_dir()?.join("artifacts").join("visual");
            *artifacts = Some(
                ArtifactStore::open(root, VISUAL_ARTIFACT_BUDGET_BYTES)
                    .await
                    .map_err(|_| "responsive_contact_sheet_failed".to_string())?,
            );
        }
        let artifacts = artifacts
            .as_mut()
            .expect("visual artifact store initialized above");
        state
            .retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, artifacts.used_bytes())
            .map_err(|_| "responsive_memory_budget_exceeded".to_string())?;
        let projected = artifacts
            .projected_used_bytes_after_put(&png)
            .map_err(|_| "responsive_memory_budget_exceeded".to_string())?;
        state
            .retained_resources
            .admit_projected(RetainedResourceKind::CaptureStorage, projected)
            .map_err(|_| "responsive_memory_budget_exceeded".to_string())?;
        let put = artifacts.put("visual/png", &png).await;
        let actual = artifacts.used_bytes();
        let reconcile = state
            .retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, actual);
        let artifact = put.map_err(|_| "responsive_contact_sheet_failed".to_string())?;
        reconcile.map_err(|_| "responsive_memory_budget_exceeded".to_string())?;
        artifact.id
    };
    drop(png);

    if captured.len() != plan.presets.len() || captured.len() != geometry.placements.len() {
        return Err("responsive_contact_sheet_failed".into());
    }
    let revision = captured.first().and_then(|entry| entry.revision.clone());
    if captured.iter().any(|entry| entry.revision != revision || entry.route != route) {
        return Err("responsive_route_drift".into());
    }
    let captured_at_unix_ms = captured
        .iter()
        .map(|entry| entry.captured_at_unix_ms)
        .max()
        .ok_or_else(|| "responsive_contact_sheet_failed".to_string())?;
    let captured_at_unix_ms = i64::try_from(captured_at_unix_ms)
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?;

    let viewports = captured
        .iter()
        .zip(geometry.placements.iter())
        .map(|(entry, placement)| ResponsiveViewportEvidence {
            preset: entry.responsive_frame.preset,
            css_width: entry.responsive_frame.viewport.width,
            css_height: entry.responsive_frame.viewport.height,
            device_scale_factor: entry.device_scale_factor,
            pixel_width: entry.responsive_frame.pixel_width,
            pixel_height: entry.responsive_frame.pixel_height,
            sheet_x: placement.x,
            sheet_y: placement.y,
        })
        .collect::<Vec<_>>();
    let metadata = ResponsiveVisualEvidenceRequest {
        artifact_id: artifact_id.clone(),
        route: route.to_owned(),
        revision,
        captured_at_unix_ms,
        contact_sheet_pixel_width: geometry.pixel_width,
        contact_sheet_pixel_height: geometry.pixel_height,
        viewports: viewports.clone(),
    };

    let token = read_token()
        .await
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?;
    let evidence = control_client()
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/evidence/visual-responsive"
        ))
        .bearer_auth(token)
        .json(&metadata)
        .send()
        .await
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?
        .error_for_status()
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?
        .json::<VisualEvidenceResponse>()
        .await
        .map_err(|_| "responsive_contact_sheet_failed".to_string())?;

    Ok(ResponsiveSweepReceipt {
        artifact_id,
        evidence_id: evidence.evidence_id,
        deduplicated: evidence.deduplicated,
        route: route.to_owned(),
        contact_sheet_pixel_width: geometry.pixel_width,
        contact_sheet_pixel_height: geometry.pixel_height,
        viewports: viewports
            .into_iter()
            .map(|viewport| ResponsiveViewportReceipt {
                preset: viewport.preset,
                css_width: viewport.css_width,
                css_height: viewport.css_height,
                device_scale_factor: viewport.device_scale_factor,
                pixel_width: viewport.pixel_width,
                pixel_height: viewport.pixel_height,
                sheet_x: viewport.sheet_x,
                sheet_y: viewport.sheet_y,
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn capture_full_page(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<FullPageCaptureReceipt, String> {
    validate_viewport(&viewport).map_err(|_| "full_page_invalid_geometry".to_string())?;
    preflight_managed_surface(&app, session_id)
        .map_err(|_| "full_page_managed_surface_unavailable".to_string())?;

    let capture_gate = session_capture_gate(&state, session_id)
        .await
        .map_err(|_| "full_page_capture_gate_unavailable".to_string())?;
    let _capture_guard = capture_gate.lock().await;
    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(FULL_PAGE_TRANSACTION_TIMEOUT_MS);

    full_page_capture_after_gate(app, &state, session_id, viewport, revision, deadline).await
}

async fn full_page_capture_after_gate(
    app: tauri::AppHandle,
    state: &VisualCaptureState,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
    deadline: tokio::time::Instant,
) -> Result<FullPageCaptureReceipt, String> {
    let expected_route = managed_surface_canonical_route(&app, session_id)
        .map_err(|_| "full_page_route_drift".to_string())?;
    let work_deadline =
        deadline - Duration::from_millis(FULL_PAGE_CLEANUP_RESERVE_MS);

    tokio::time::timeout_at(work_deadline, wait_for_capture_settle(session_id))
        .await
        .map_err(|_| "full_page_transaction_timeout".to_string())?
        .map_err(|_| "full_page_settle_failed".to_string())?;

    let freeze = tokio::time::timeout_at(work_deadline, freeze_full_page_visual_state(session_id))
        .await
        .map_err(|_| "full_page_transaction_timeout".to_string())?
        .map_err(|_| "full_page_visual_freeze_failed".to_string())?;

    let work = tokio::time::timeout_at(work_deadline, async {
        validate_full_page_freeze_context(&freeze, &viewport)?;
        let plan = plan_full_page(
            freeze.document_css_width,
            freeze.document_css_height,
            freeze.viewport_css_width,
            freeze.viewport_css_height,
            freeze.scroll_y,
            FullPagePolicy::default(),
        )
        .map_err(full_page_plan_error)?;

        let frame = capture_full_page_tiles(
            &app,
            session_id,
            &viewport,
            &revision,
            &freeze,
            &plan,
            &expected_route,
        )
        .await?;

        Ok::<FullPageTransactionOutput, String>(FullPageTransactionOutput { frame, plan })
    })
    .await
    .unwrap_or_else(|_| Err("full_page_transaction_timeout".to_string()));

    let cleanup =
        cleanup_full_page_state(session_id, &viewport, &freeze, deadline).await;

    let transaction = match (work, cleanup) {
        (Ok(transaction), Ok(())) => transaction,
        (Err(primary), Ok(())) => return Err(primary),
        (Ok(_), Err(cleanup_error)) => return Err(cleanup_error),
        (Err(primary), Err(cleanup_error)) => {
            return Err(format!("{primary};{cleanup_error}"));
        }
    };

    if tokio::time::Instant::now() >= deadline {
        return Err("full_page_transaction_timeout".into());
    }

    let png = encode_png_rgba(&transaction.frame.image)
        .map_err(|_| "full_page_final_encode_failed".to_string())?;
    if tokio::time::Instant::now() >= deadline {
        return Err("full_page_transaction_timeout".into());
    }

    tokio::time::timeout_at(
        deadline,
        persist_full_page_and_register(
            state,
            session_id,
            png,
            &transaction.frame,
            &transaction.plan,
        ),
    )
    .await
    .map_err(|_| "full_page_transaction_timeout".to_string())?
}

async fn capture_full_page_tiles(
    app: &tauri::AppHandle,
    session_id: SessionId,
    viewport: &ViewportMeta,
    revision: &Option<String>,
    freeze: &FullPageFreezeVisualStateReceipt,
    plan: &FullPagePlan,
    expected_route: &str,
) -> Result<FullPageStitchedFrame, String> {
    let mut output: Option<RgbaImage> = None;
    let mut backend: Option<String> = None;
    let mut pixel_width: Option<u32> = None;
    let mut pixel_height: Option<u32> = None;
    let mut captured_at_unix_ms = 0_u64;

    for requested_y in plan.scroll_offsets_y.iter().copied() {
        let scroll = capture_scroll_to(session_id, &freeze.token, requested_y)
            .await
            .map_err(|_| "full_page_scroll_failed".to_string())?;
        validate_capture_scroll_receipt(&scroll, freeze, viewport, requested_y)?;

        wait_for_capture_settle(session_id)
            .await
            .map_err(|_| "full_page_settle_failed".to_string())?;

        let probe = capture_tile_probe(session_id, &freeze.token)
            .await
            .map_err(|_| "full_page_tile_probe_failed".to_string())?;
        validate_capture_tile_probe(&probe, freeze, viewport, &scroll)?;
        if probe.visible_fixed_or_sticky {
            return Err("full_page_fixed_or_sticky_unsupported".into());
        }

        let frame = capture_managed_surface(
            app,
            session_id,
            viewport.clone(),
            revision.clone(),
        )
        .await
        .map_err(|_| "full_page_native_capture_failed".to_string())?;

        let canonical_route = canonical_visual_diff_route(&frame.route)
            .map_err(|_| "full_page_route_drift".to_string())?;
        if canonical_route != expected_route {
            return Err("full_page_route_drift".into());
        }
        if frame.viewport.css_width != viewport.css_width
            || frame.viewport.css_height != viewport.css_height
            || frame.viewport.device_scale_factor != viewport.device_scale_factor
        {
            return Err("full_page_viewport_geometry_drift".into());
        }
        if frame.revision.as_ref() != revision.as_ref() {
            return Err("full_page_revision_drift".into());
        }

        let frame_backend = frame.backend.to_string();
        match backend.as_deref() {
            Some(expected) if expected != frame_backend => {
                return Err("full_page_native_geometry_drift".into());
            }
            None => backend = Some(frame_backend.clone()),
            _ => {}
        }
        match (pixel_width, pixel_height) {
            (Some(width), Some(height))
                if width != frame.pixel_width || height != frame.pixel_height =>
            {
                return Err("full_page_native_geometry_drift".into());
            }
            (None, None) => {
                pixel_width = Some(frame.pixel_width);
                pixel_height = Some(frame.pixel_height);
            }
            _ => return Err("full_page_native_geometry_drift".into()),
        }

        let (redacted_png, applied) = localview_visual::redact_png_css_rects(
            &frame.png,
            (frame.pixel_width, frame.pixel_height),
            (probe.viewport_css_width, probe.viewport_css_height),
            &probe.mask_rects,
        )
        .map_err(|_| "full_page_tile_redaction_failed".to_string())?;
        if applied != probe.mask_rects.len() {
            return Err("full_page_tile_redaction_failed".into());
        }

        let tile = decode_png_rgba(&redacted_png)
            .map_err(|_| "full_page_tile_decode_failed".to_string())?;
        if (tile.width, tile.height) != (frame.pixel_width, frame.pixel_height) {
            return Err("full_page_native_geometry_drift".into());
        }

        if output.is_none() {
            let geometry = project_full_page_output(
                plan,
                tile.width,
                tile.height,
                FullPagePolicy::default(),
            )
            .map_err(full_page_plan_error)?;
            let mut data = Vec::new();
            data.try_reserve_exact(geometry.rgba_bytes)
                .map_err(|_| "full_page_output_memory_budget_exceeded".to_string())?;
            data.resize(geometry.rgba_bytes, 0);
            output = Some(RgbaImage {
                width: geometry.pixel_width,
                height: geometry.pixel_height,
                data,
            });
        }

        stitch_full_page_tile(
            output.as_mut().expect("full-page output initialized above"),
            probe.viewport_css_height,
            scroll.actual_y,
            &tile,
        )
        .map_err(|_| "full_page_stitch_failed".to_string())?;

        captured_at_unix_ms = frame.captured_at_unix_ms;
    }

    let image = output.ok_or_else(|| "full_page_invalid_geometry".to_string())?;
    Ok(FullPageStitchedFrame {
        image,
        backend: backend.ok_or_else(|| "full_page_native_geometry_drift".to_string())?,
        route: expected_route.to_owned(),
        viewport: viewport.clone(),
        revision: revision.clone(),
        captured_at_unix_ms,
    })
}

async fn cleanup_full_page_state(
    session_id: SessionId,
    viewport: &ViewportMeta,
    freeze: &FullPageFreezeVisualStateReceipt,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    let original_scroll_y = freeze.scroll_y;
    let scroll_restore = match tokio::time::timeout_at(
        deadline,
        capture_scroll_to(session_id, &freeze.token, original_scroll_y),
    )
    .await
    {
        Ok(Ok(receipt)) => validate_capture_scroll_receipt(
            &receipt,
            freeze,
            viewport,
            original_scroll_y,
        )
        .is_ok(),
        _ => false,
    };

    let visual_restore =
        match tokio::time::timeout_at(deadline, restore_visual_state(session_id, &freeze.token))
            .await
        {
            Ok(Ok(())) => true,
            _ => false,
        };

    match (scroll_restore, visual_restore) {
        (true, true) => Ok(()),
        (false, true) => Err("full_page_scroll_restore_failed".into()),
        (true, false) => Err("full_page_visual_restore_failed".into()),
        (false, false) => {
            Err("full_page_scroll_restore_failed;full_page_visual_restore_failed".into())
        }
    }
}

async fn persist_full_page_and_register(
    state: &VisualCaptureState,
    session_id: SessionId,
    png: Vec<u8>,
    frame: &FullPageStitchedFrame,
    plan: &FullPagePlan,
) -> Result<FullPageCaptureReceipt, String> {
    let artifact_id = {
        let mut artifacts = state.artifacts.lock().await;
        if artifacts.is_none() {
            let root = state_dir()?.join("artifacts").join("visual");
            *artifacts = Some(
                ArtifactStore::open(root, VISUAL_ARTIFACT_BUDGET_BYTES)
                    .await
                    .map_err(|_| "full_page_artifact_store_failed".to_string())?,
            );
        }
        let artifacts = artifacts
            .as_mut()
            .expect("visual artifact store initialized above");
        let retained_resources = &state.retained_resources;

        retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, artifacts.used_bytes())
            .map_err(|_| "full_page_artifact_budget_denied".to_string())?;
        let projected_bytes = artifacts
            .projected_used_bytes_after_put(&png)
            .map_err(|_| "full_page_artifact_budget_denied".to_string())?;
        retained_resources
            .admit_projected(RetainedResourceKind::CaptureStorage, projected_bytes)
            .map_err(|_| "full_page_artifact_budget_denied".to_string())?;

        let put_result = artifacts.put("visual/png", &png).await;
        let actual_bytes = artifacts.used_bytes();
        let reconcile_result = retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, actual_bytes);

        let artifact = put_result.map_err(|_| "full_page_artifact_persist_failed".to_string())?;
        reconcile_result.map_err(|_| "full_page_artifact_budget_denied".to_string())?;
        artifact.id
    };
    drop(png);

    let captured_at_for_api = i64::try_from(frame.captured_at_unix_ms)
        .map_err(|_| "full_page_capture_timestamp_invalid".to_string())?;
    let metadata = FullPageVisualEvidenceRequest {
        artifact_id: artifact_id.clone(),
        pixel_width: frame.image.width,
        pixel_height: frame.image.height,
        backend: frame.backend.clone(),
        route: frame.route.clone(),
        viewport: frame.viewport.clone(),
        revision: frame.revision.clone(),
        captured_at_unix_ms: captured_at_for_api,
        document_css_width: plan.document_css_width,
        document_css_height: plan.document_css_height,
        tile_count: plan.scroll_offsets_y.len(),
        scroll_offsets_y: plan.scroll_offsets_y.clone(),
    };

    let token = read_token()
        .await
        .map_err(|_| "full_page_evidence_registration_failed".to_string())?;
    let evidence = control_client()
        .map_err(|_| "full_page_evidence_registration_failed".to_string())?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/evidence/visual-full-page"
        ))
        .bearer_auth(token)
        .json(&metadata)
        .send()
        .await
        .map_err(|_| "full_page_evidence_registration_failed".to_string())?
        .error_for_status()
        .map_err(|_| "full_page_evidence_registration_failed".to_string())?
        .json::<VisualEvidenceResponse>()
        .await
        .map_err(|_| "full_page_evidence_registration_failed".to_string())?;

    Ok(FullPageCaptureReceipt {
        artifact_id,
        evidence_id: evidence.evidence_id,
        deduplicated: evidence.deduplicated,
        backend: frame.backend.clone(),
        route: frame.route.clone(),
        viewport: frame.viewport.clone(),
        pixel_width: frame.image.width,
        pixel_height: frame.image.height,
        revision: frame.revision.clone(),
        captured_at_unix_ms: frame.captured_at_unix_ms,
        document_css_width: plan.document_css_width,
        document_css_height: plan.document_css_height,
        tile_count: plan.scroll_offsets_y.len(),
    })
}

async fn freeze_full_page_visual_state(
    session_id: SessionId,
) -> Result<FullPageFreezeVisualStateReceipt, String> {
    let token = read_token().await?;
    let receipt = control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-freeze-full-page"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<FullPageFreezeVisualStateReceipt>()
        .await
        .map_err(err)?;

    if !valid_full_page_freeze_receipt(&receipt) {
        return Err("invalid full-page visual freeze acknowledgement".into());
    }
    let _ = receipt.web_animations_supported;
    Ok(receipt)
}

async fn capture_scroll_to(
    session_id: SessionId,
    token: &str,
    y: f64,
) -> Result<CaptureScrollReceipt, String> {
    let control_token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-scroll"
        ))
        .bearer_auth(control_token)
        .json(&serde_json::json!({"token": token, "y": y}))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<CaptureScrollReceipt>()
        .await
        .map_err(err)
}

async fn capture_tile_probe(
    session_id: SessionId,
    token: &str,
) -> Result<CaptureTileProbeReceipt, String> {
    let control_token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-tile-probe"
        ))
        .bearer_auth(control_token)
        .json(&serde_json::json!({"token": token}))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<CaptureTileProbeReceipt>()
        .await
        .map_err(err)
}

fn valid_full_page_freeze_receipt(receipt: &FullPageFreezeVisualStateReceipt) -> bool {
    let policy = FullPagePolicy::default();
    if receipt.token.is_empty()
        || receipt.paused_animations > MAX_PAUSED_ANIMATIONS
        || receipt.lease_ms != FULL_PAGE_VISUAL_FREEZE_LEASE_MS
        || receipt.masked_elements > MAX_MASKED_ELEMENTS
        || receipt.mask_rects.len() > MAX_VISUAL_MASK_RECTS
        || !receipt.viewport_css_width.is_finite()
        || !receipt.viewport_css_height.is_finite()
        || !receipt.document_css_width.is_finite()
        || !receipt.document_css_height.is_finite()
        || !receipt.scroll_x.is_finite()
        || !receipt.scroll_y.is_finite()
        || receipt.viewport_css_width <= 0.0
        || receipt.viewport_css_height <= 0.0
        || receipt.document_css_width <= 0.0
        || receipt.document_css_height <= 0.0
        || receipt.scroll_x < 0.0
        || receipt.scroll_y < 0.0
        || receipt.viewport_css_width > MAX_CSS_VIEWPORT_DIMENSION
        || receipt.viewport_css_height > MAX_CSS_VIEWPORT_DIMENSION
        || receipt.document_css_height > policy.max_document_css_height
        || receipt.document_css_width != receipt.viewport_css_width
    {
        return false;
    }

    valid_full_page_mask_rects(
        receipt.masked_elements,
        &receipt.mask_rects,
        receipt.viewport_css_width,
        receipt.viewport_css_height,
    )
}

fn validate_full_page_freeze_context(
    freeze: &FullPageFreezeVisualStateReceipt,
    viewport: &ViewportMeta,
) -> Result<(), String> {
    if !valid_full_page_freeze_receipt(freeze)
        || freeze.viewport_css_width != f64::from(viewport.css_width)
        || freeze.viewport_css_height != f64::from(viewport.css_height)
    {
        return Err("full_page_viewport_geometry_drift".into());
    }
    let tolerance = full_page_scroll_tolerance(viewport);
    let max_scroll_y = (freeze.document_css_height - freeze.viewport_css_height).max(0.0);
    if freeze.scroll_x > tolerance || freeze.scroll_y > max_scroll_y + tolerance {
        return Err("full_page_scroll_mismatch".into());
    }
    Ok(())
}

fn validate_capture_scroll_receipt(
    receipt: &CaptureScrollReceipt,
    freeze: &FullPageFreezeVisualStateReceipt,
    viewport: &ViewportMeta,
    requested_y: f64,
) -> Result<(), String> {
    let values = [
        receipt.requested_y,
        receipt.actual_x,
        receipt.actual_y,
        receipt.document_css_width,
        receipt.document_css_height,
        receipt.viewport_css_width,
        receipt.viewport_css_height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || receipt.requested_y < 0.0
        || receipt.actual_x < 0.0
        || receipt.actual_y < 0.0
    {
        return Err("full_page_scroll_mismatch".into());
    }
    if receipt.document_css_width != freeze.document_css_width
        || receipt.document_css_height != freeze.document_css_height
    {
        return Err("full_page_document_geometry_drift".into());
    }
    if receipt.viewport_css_width != freeze.viewport_css_width
        || receipt.viewport_css_height != freeze.viewport_css_height
    {
        return Err("full_page_viewport_geometry_drift".into());
    }

    let tolerance = full_page_scroll_tolerance(viewport);
    if receipt.requested_y != requested_y
        || (receipt.actual_x - freeze.scroll_x).abs() > tolerance
        || (receipt.actual_y - requested_y).abs() > tolerance
    {
        return Err("full_page_scroll_mismatch".into());
    }
    Ok(())
}

fn validate_capture_tile_probe(
    probe: &CaptureTileProbeReceipt,
    freeze: &FullPageFreezeVisualStateReceipt,
    viewport: &ViewportMeta,
    scroll: &CaptureScrollReceipt,
) -> Result<(), String> {
    let values = [
        probe.scroll_x,
        probe.scroll_y,
        probe.document_css_width,
        probe.document_css_height,
        probe.viewport_css_width,
        probe.viewport_css_height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || probe.scroll_x < 0.0
        || probe.scroll_y < 0.0
    {
        return Err("full_page_scroll_mismatch".into());
    }
    if probe.document_css_width != freeze.document_css_width
        || probe.document_css_height != freeze.document_css_height
    {
        return Err("full_page_document_geometry_drift".into());
    }
    if probe.viewport_css_width != freeze.viewport_css_width
        || probe.viewport_css_height != freeze.viewport_css_height
    {
        return Err("full_page_viewport_geometry_drift".into());
    }
    if probe.positional_elements_scanned > MAX_POSITIONAL_SCAN_ELEMENTS {
        return Err("full_page_positional_scan_budget_exceeded".into());
    }
    if !valid_full_page_mask_rects(
        probe.masked_elements,
        &probe.mask_rects,
        probe.viewport_css_width,
        probe.viewport_css_height,
    ) {
        return Err("full_page_private_mask_budget_exceeded".into());
    }

    let tolerance = full_page_scroll_tolerance(viewport);
    if (probe.scroll_x - scroll.actual_x).abs() > tolerance
        || (probe.scroll_y - scroll.actual_y).abs() > tolerance
    {
        return Err("full_page_scroll_mismatch".into());
    }
    Ok(())
}

fn valid_full_page_mask_rects(
    masked_elements: u64,
    rects: &[Rect],
    viewport_css_width: f64,
    viewport_css_height: f64,
) -> bool {
    if masked_elements > MAX_MASKED_ELEMENTS
        || rects.len() > MAX_VISUAL_MASK_RECTS
        || !viewport_css_width.is_finite()
        || !viewport_css_height.is_finite()
        || viewport_css_width <= 0.0
        || viewport_css_height <= 0.0
    {
        return false;
    }
    rects.iter().all(|rect| {
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
            && right <= viewport_css_width
            && bottom <= viewport_css_height
    })
}

fn full_page_scroll_tolerance(viewport: &ViewportMeta) -> f64 {
    (1.0 / viewport.device_scale_factor).max(0.01)
}

fn full_page_plan_error(error: FullPageError) -> String {
    match error {
        FullPageError::DocumentTooTall => "full_page_document_too_tall",
        FullPageError::TileBudgetExceeded => "full_page_tile_budget_exceeded",
        FullPageError::OutputMemoryBudgetExceeded => "full_page_output_memory_budget_exceeded",
        FullPageError::OutputPixelHeightExceeded => "full_page_output_pixel_height_exceeded",
        FullPageError::InvalidImage | FullPageError::PlacementOutOfBounds => {
            "full_page_stitch_failed"
        }
        FullPageError::InvalidGeometry
        | FullPageError::InvalidPolicy
        | FullPageError::WidthMismatch
        | FullPageError::ArithmeticOverflow => "full_page_invalid_geometry",
    }
    .to_string()
}

pub(crate) fn managed_surface_canonical_route(
    app: &tauri::AppHandle,
    session_id: SessionId,
) -> Result<String, String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !bridge_surface_label_allowed(window.label(), session_id) {
            return Err("managed surface ownership mismatch".into());
        }
        let route = window.url().map_err(err)?;
        if !workspace_navigation_allowed(&route) {
            return Err("managed surface route is not loopback".into());
        }
        return canonical_visual_diff_route(route.as_str());
    }

    #[cfg(feature = "native-workspace")]
    {
        let workspace_label = workspace_surface::workspace_label(session_id);
        if let Some(window) = app.get_webview(&workspace_label) {
            if !bridge_surface_label_allowed(window.label(), session_id) {
                return Err("managed surface ownership mismatch".into());
            }
            let route = window.url().map_err(err)?;
            if !workspace_navigation_allowed(&route) {
                return Err("managed surface route is not loopback".into());
            }
            return canonical_visual_diff_route(route.as_str());
        }
    }

    Err("managed surface unavailable".into())
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

#[derive(Debug, Clone)]
pub(crate) struct VerificationVisualFrame {
    pub png: Vec<u8>,
    pub viewport: ViewportMeta,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub backend: NativeCaptureBackend,
    pub route: String,
    pub revision: Option<String>,
    pub captured_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct VerificationAffectedVisualEvidence {
    pub visual_diff_evidence_id: String,
    pub visual_evidence_ids: Vec<String>,
}

async fn capture_current_redacted_frame(
    app: tauri::AppHandle,
    state: &VisualCaptureState,
    session_id: SessionId,
    revision: Option<String>,
) -> Result<CapturedFrame, String> {
    preflight_managed_surface(&app, session_id)?;

    let capture_gate = session_capture_gate(state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;

    wait_for_capture_settle(session_id).await?;
    let freeze = freeze_visual_state(session_id).await?;

    let capture_result = match trusted_viewport_from_freeze(&app, session_id, &freeze) {
        Ok(viewport) => {
            let expected_scale_factor = viewport.device_scale_factor;
            capture_managed_surface(&app, session_id, viewport, revision)
                .await
                .and_then(|frame| {
                    validate_trusted_current_viewport(
                        &app,
                        session_id,
                        &frame,
                        &freeze,
                        expected_scale_factor,
                    )?;
                    Ok(frame)
                })
        }
        Err(error) => Err(error),
    };
    let restore_result = restore_visual_state(session_id, &freeze.token).await;

    let frame = match (capture_result, restore_result) {
        (Ok(frame), Ok(())) => frame,
        (Err(capture_error), Ok(())) => return Err(capture_error),
        (Ok(_), Err(_)) | (Err(_), Err(_)) => {
            return Err(
                "trusted current viewport restore acknowledgement failed; pixels discarded".into(),
            );
        }
    };

    redact_private_pixels(frame, &freeze)
}

fn verification_visual_frame(frame: CapturedFrame) -> VerificationVisualFrame {
    VerificationVisualFrame {
        png: frame.png,
        viewport: frame.viewport,
        pixel_width: frame.pixel_width,
        pixel_height: frame.pixel_height,
        backend: frame.backend,
        route: frame.route,
        revision: frame.revision,
        captured_at_unix_ms: frame.captured_at_unix_ms,
    }
}

pub(crate) async fn capture_verification_baseline(
    app: tauri::AppHandle,
    state: &VisualCaptureState,
    session_id: SessionId,
) -> Result<VerificationVisualFrame, String> {
    capture_current_redacted_frame(app, state, session_id, None)
        .await
        .map(verification_visual_frame)
}

pub(crate) async fn capture_verification_current(
    app: tauri::AppHandle,
    state: &VisualCaptureState,
    session_id: SessionId,
) -> Result<VerificationVisualFrame, String> {
    capture_current_redacted_frame(app, state, session_id, None)
        .await
        .map(verification_visual_frame)
}

pub(crate) async fn persist_verification_affected_visual_evidence(
    state: &VisualCaptureState,
    session_id: SessionId,
    frame: &VerificationVisualFrame,
    mode: &str,
    regions: &[Rect],
    changed_ratio: f64,
) -> Result<VerificationAffectedVisualEvidence, String> {
    if !changed_ratio.is_finite() || !(0.0..=1.0).contains(&changed_ratio) {
        return Err("trusted Verify affected visual ratio is invalid".into());
    }

    let mode = match mode {
        "unchanged" => "unchanged",
        "regions" => "regions",
        "viewport" => "viewport",
        _ => return Err("trusted Verify affected visual mode is invalid".into()),
    };
    let mut visual_evidence_ids = Vec::new();

    match mode {
        "unchanged" => {
            if changed_ratio != 0.0 || !regions.is_empty() {
                return Err("trusted Verify unchanged visual plan is inconsistent".into());
            }
        }
        "regions" => {
            let max_regions = ChangedRegionPolicy::default().max_regions;
            if regions.is_empty() || regions.len() > max_regions {
                return Err("trusted Verify affected region count is invalid".into());
            }
            let image = decode_png_rgba(&frame.png)
                .map_err(|_| "trusted Verify current visual decode failed".to_string())?;
            if (image.width, image.height) != (frame.pixel_width, frame.pixel_height) {
                return Err("trusted Verify current visual pixel metadata mismatch".into());
            }
            for rect in regions {
                validate_region(
                    rect,
                    f64::from(frame.viewport.css_width),
                    f64::from(frame.viewport.css_height),
                )?;
                let cropped = image
                    .crop_css_rect(
                        (
                            f64::from(frame.viewport.css_width),
                            f64::from(frame.viewport.css_height),
                        ),
                        rect,
                    )
                    .map_err(|_| "trusted Verify affected region crop failed".to_string())?;
                let png = encode_png_rgba(&cropped)
                    .map_err(|_| "trusted Verify affected region encode failed".to_string())?;
                let region_frame = CapturedFrame {
                    png,
                    pixel_width: cropped.width,
                    pixel_height: cropped.height,
                    backend: frame.backend,
                    viewport: frame.viewport.clone(),
                    route: frame.route.clone(),
                    revision: frame.revision.clone(),
                    captured_at_unix_ms: frame.captured_at_unix_ms,
                };
                let receipt = persist_and_register(
                    state,
                    session_id,
                    region_frame,
                    &RequestedCaptureTarget::Region(rect.clone()),
                )
                .await?;
                visual_evidence_ids.push(receipt.evidence_id);
            }
        }
        "viewport" => {
            if regions.len() != 1 {
                return Err("trusted Verify viewport visual plan is inconsistent".into());
            }
            let expected = Rect {
                x: 0.0,
                y: 0.0,
                width: f64::from(frame.viewport.css_width),
                height: f64::from(frame.viewport.css_height),
            };
            if regions[0] != expected {
                return Err("trusted Verify viewport visual plan geometry is invalid".into());
            }
            let viewport_frame = CapturedFrame {
                png: frame.png.clone(),
                pixel_width: frame.pixel_width,
                pixel_height: frame.pixel_height,
                backend: frame.backend,
                viewport: frame.viewport.clone(),
                route: frame.route.clone(),
                revision: frame.revision.clone(),
                captured_at_unix_ms: frame.captured_at_unix_ms,
            };
            let receipt = persist_and_register(
                state,
                session_id,
                viewport_frame,
                &RequestedCaptureTarget::Viewport,
            )
            .await?;
            visual_evidence_ids.push(receipt.evidence_id);
        }
        _ => unreachable!("trusted Verify visual mode validated above"),
    }

    let visual_diff = register_visual_diff_evidence(
        session_id,
        frame.route.clone(),
        frame.viewport.clone(),
        frame.revision.clone(),
        frame.captured_at_unix_ms,
        mode,
        changed_ratio,
        visual_evidence_ids.clone(),
    )
    .await?;

    Ok(VerificationAffectedVisualEvidence {
        visual_diff_evidence_id: visual_diff.evidence_id,
        visual_evidence_ids,
    })
}

#[tauri::command]
pub async fn capture_current_viewport(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    revision: Option<String>,
) -> Result<VisualCaptureReceipt, String> {
    let frame = capture_current_redacted_frame(app, &state, session_id, revision).await?;
    persist_and_register(
        &state,
        session_id,
        frame,
        &RequestedCaptureTarget::Viewport,
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

#[tauri::command]
pub async fn capture_changed_regions(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<ChangedRegionCaptureReceipt, String> {
    validate_viewport(&viewport)?;
    preflight_managed_surface(&app, session_id)?;

    let capture_gate = session_capture_gate(&state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;

    let (frame, freeze) =
        capture_redacted_viewport_after_gate(app, session_id, viewport, revision).await?;
    validate_changed_viewport(&frame, &freeze)?;

    let image = Arc::new(
        decode_png_rgba(&frame.png)
            .map_err(|_| "changed-region visual decode failed; pixels discarded".to_string())?,
    );
    if (image.width, image.height) != (frame.pixel_width, frame.pixel_height) {
        return Err("changed-region native pixel metadata mismatch; pixels discarded".into());
    }

    let context = changed_baseline_context(&frame);
    let baseline = compatible_changed_baseline(&state, session_id, &context).await?;
    let baseline_reset = baseline.is_none();
    let plan = match baseline.as_deref() {
        Some(before) => plan_changed_css_regions(
            before,
            image.as_ref(),
            (freeze.viewport_css_width, freeze.viewport_css_height),
            ChangedRegionPolicy::default(),
        )
        .map_err(|_| "changed-region visual planning failed; pixels discarded".to_string())?,
        None => ChangedRegionPlan::Viewport {
            changed_ratio: 1.0,
        },
    };

    if let ChangedRegionPlan::Unchanged = &plan {
        let visual_diff = register_visual_diff_evidence(
            session_id,
            frame.route.clone(),
            frame.viewport.clone(),
            frame.revision.clone(),
            frame.captured_at_unix_ms,
            "unchanged",
            0.0,
            Vec::new(),
        )
        .await?;
        return Ok(ChangedRegionCaptureReceipt {
            mode: "unchanged",
            changed_ratio: 0.0,
            receipts: Vec::new(),
            visual_diff_evidence_id: visual_diff.evidence_id,
            visual_diff_deduplicated: visual_diff.deduplicated,
            baseline_cached: true,
        });
    }

    let diff_route = frame.route.clone();
    let diff_viewport = frame.viewport.clone();
    let diff_revision = frame.revision.clone();
    let diff_captured_at_unix_ms = frame.captured_at_unix_ms;
    let emission = emit_changed_capture_plan(
        &state,
        session_id,
        frame,
        image.as_ref(),
        &freeze,
        &plan,
        baseline_reset,
    )
    .await?;
    let visual_evidence_ids = emission
        .receipts
        .iter()
        .map(|receipt| receipt.evidence_id.clone())
        .collect();
    let visual_diff = register_visual_diff_evidence(
        session_id,
        diff_route,
        diff_viewport,
        diff_revision,
        diff_captured_at_unix_ms,
        emission.mode,
        emission.changed_ratio,
        visual_evidence_ids,
    )
    .await?;
    let baseline_cached = commit_changed_baseline(&state, session_id, context, image).await?;

    Ok(ChangedRegionCaptureReceipt {
        mode: emission.mode,
        changed_ratio: emission.changed_ratio,
        receipts: emission.receipts,
        visual_diff_evidence_id: visual_diff.evidence_id,
        visual_diff_deduplicated: visual_diff.deduplicated,
        baseline_cached,
    })
}

async fn capture_redacted_viewport_after_gate(
    app: tauri::AppHandle,
    session_id: SessionId,
    viewport: ViewportMeta,
    revision: Option<String>,
) -> Result<(CapturedFrame, FreezeVisualStateReceipt), String> {
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
    let frame = redact_private_pixels(frame, &freeze)?;
    Ok((frame, freeze))
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

    let (frame, freeze) =
        capture_redacted_viewport_after_gate(app, session_id, viewport, revision).await?;
    validate_live_target_viewport(&frame, &freeze, &target)?;
    let frame = apply_capture_target(frame, &freeze, &target)?;

    persist_and_register(&state, session_id, frame, &target).await
}

async fn compatible_changed_baseline(
    state: &VisualCaptureState,
    session_id: SessionId,
    context: &VisualBaselineContext,
) -> Result<Option<Arc<RgbaImage>>, String> {
    let mut baselines = state.baselines.lock().await;
    if baselines.is_none() {
        *baselines = Some(
            VisualBaselineCache::new(VISUAL_BASELINE_BUDGET_BYTES, MAX_VISUAL_BASELINES)
                .map_err(|_| "visual baseline cache policy is invalid".to_string())?,
        );
    }
    let baselines = baselines
        .as_mut()
        .expect("visual baseline cache initialized above");
    let retained_resources = &state.retained_resources;

    let current_bytes = u64::try_from(baselines.used_bytes())
        .map_err(|_| "visual baseline retained usage exceeds supported accounting range".to_string())?;
    retained_resources
        .synchronize(RetainedResourceKind::Cache, current_bytes)
        .map_err(retained_resource_error)?;
    let compatible = baselines.get_compatible(session_id, context);
    let actual_bytes = u64::try_from(baselines.used_bytes())
        .map_err(|_| "visual baseline retained usage exceeds supported accounting range".to_string())?;
    retained_resources
        .synchronize(RetainedResourceKind::Cache, actual_bytes)
        .map_err(retained_resource_error)?;
    Ok(compatible)
}

async fn commit_changed_baseline(
    state: &VisualCaptureState,
    session_id: SessionId,
    context: VisualBaselineContext,
    image: Arc<RgbaImage>,
) -> Result<bool, String> {
    let mut baselines = state.baselines.lock().await;
    if baselines.is_none() {
        *baselines = Some(
            VisualBaselineCache::new(VISUAL_BASELINE_BUDGET_BYTES, MAX_VISUAL_BASELINES)
                .map_err(|_| "visual baseline cache policy is invalid".to_string())?,
        );
    }
    let baselines = baselines
        .as_mut()
        .expect("visual baseline cache initialized above");
    let retained_resources = &state.retained_resources;

    let current_bytes = u64::try_from(baselines.used_bytes())
        .map_err(|_| "visual baseline retained usage exceeds supported accounting range".to_string())?;
    retained_resources
        .synchronize(RetainedResourceKind::Cache, current_bytes)
        .map_err(retained_resource_error)?;

    image
        .validate()
        .map_err(|_| "visual baseline cache rejected the captured frame".to_string())?;
    if context.pixel_width != image.width || context.pixel_height != image.height {
        return Err("visual baseline cache rejected the captured frame".to_string());
    }
    let Some(projected_bytes) = baselines
        .projected_used_bytes_after_insert(session_id, image.data.len())
        .map_err(|_| "visual baseline cache rejected the captured frame".to_string())?
    else {
        return Ok(false);
    };
    let projected_bytes = u64::try_from(projected_bytes)
        .map_err(|_| "visual baseline retained projection exceeds supported accounting range".to_string())?;
    retained_resources
        .admit_projected(RetainedResourceKind::Cache, projected_bytes)
        .map_err(retained_resource_error)?;

    let insert_result = baselines.insert(session_id, context, image);
    let actual_bytes = u64::try_from(baselines.used_bytes())
        .map_err(|_| "visual baseline retained usage exceeds supported accounting range".to_string())?;
    let reconcile_result = retained_resources.synchronize(RetainedResourceKind::Cache, actual_bytes);

    let cached = insert_result
        .map_err(|_| "visual baseline cache rejected the captured frame".to_string())?;
    reconcile_result.map_err(retained_resource_error)?;
    Ok(cached)
}

fn changed_baseline_context(frame: &CapturedFrame) -> VisualBaselineContext {
    VisualBaselineContext {
        route: frame.route.clone(),
        css_width: frame.viewport.css_width,
        css_height: frame.viewport.css_height,
        device_scale_factor: frame.viewport.device_scale_factor,
        pixel_width: frame.pixel_width,
        pixel_height: frame.pixel_height,
    }
}

fn validate_changed_viewport(
    frame: &CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
) -> Result<(), String> {
    if frame.viewport.css_width as f64 != freeze.viewport_css_width
        || frame.viewport.css_height as f64 != freeze.viewport_css_height
    {
        return Err("changed-region viewport changed during capture; pixels discarded".into());
    }
    Ok(())
}

async fn emit_changed_capture_plan(
    state: &VisualCaptureState,
    session_id: SessionId,
    frame: CapturedFrame,
    image: &RgbaImage,
    freeze: &FreezeVisualStateReceipt,
    plan: &ChangedRegionPlan,
    baseline_reset: bool,
) -> Result<ChangedCaptureEmission, String> {
    match plan {
        ChangedRegionPlan::Unchanged => Ok(ChangedCaptureEmission {
            mode: "unchanged",
            changed_ratio: 0.0,
            receipts: Vec::new(),
        }),
        ChangedRegionPlan::Regions {
            regions,
            changed_ratio,
        } => {
            if regions.is_empty() {
                return Err("changed-region planner returned an empty region set".into());
            }

            let CapturedFrame {
                png,
                pixel_width: _,
                pixel_height: _,
                backend,
                viewport,
                route,
                revision,
                captured_at_unix_ms,
            } = frame;
            drop(png);

            let mut receipts = Vec::with_capacity(regions.len());
            for rect in regions {
                validate_region(rect, freeze.viewport_css_width, freeze.viewport_css_height)?;
                let cropped = image
                    .crop_css_rect(
                        (freeze.viewport_css_width, freeze.viewport_css_height),
                        rect,
                    )
                    .map_err(|_| {
                        "changed-region native crop failed; pixels discarded".to_string()
                    })?;
                let png = encode_png_rgba(&cropped).map_err(|_| {
                    "changed-region PNG encode failed; pixels discarded".to_string()
                })?;
                let region_frame = CapturedFrame {
                    png,
                    pixel_width: cropped.width,
                    pixel_height: cropped.height,
                    backend,
                    viewport: viewport.clone(),
                    route: route.clone(),
                    revision: revision.clone(),
                    captured_at_unix_ms,
                };
                let target = RequestedCaptureTarget::Region(rect.clone());
                receipts.push(
                    persist_and_register(state, session_id, region_frame, &target).await?,
                );
            }

            Ok(ChangedCaptureEmission {
                mode: "regions",
                changed_ratio: *changed_ratio,
                receipts,
            })
        }
        ChangedRegionPlan::Viewport { changed_ratio } => {
            let target = RequestedCaptureTarget::Viewport;
            let receipt = persist_and_register(state, session_id, frame, &target).await?;
            if baseline_reset {
                Ok(ChangedCaptureEmission {
                    mode: "baseline_reset",
                    changed_ratio: *changed_ratio,
                    receipts: vec![receipt],
                })
            } else {
                Ok(ChangedCaptureEmission {
                    mode: "viewport",
                    changed_ratio: *changed_ratio,
                    receipts: vec![receipt],
                })
            }
        }
    }
}

async fn register_visual_diff_evidence(
    session_id: SessionId,
    route: String,
    viewport: ViewportMeta,
    revision: Option<String>,
    captured_at_unix_ms: u64,
    mode: &'static str,
    changed_ratio: f64,
    visual_evidence_ids: Vec<String>,
) -> Result<VisualDiffEvidenceResponse, String> {
    if !changed_ratio.is_finite() || !(0.0..=1.0).contains(&changed_ratio) {
        return Err("visual diff ratio is outside the bounded unit interval".into());
    }
    let captured_at_unix_ms = i64::try_from(captured_at_unix_ms)
        .map_err(|_| "visual diff timestamp exceeds daemon range".to_string())?;
    let route = canonical_visual_diff_route(&route)?;
    let metadata = VisualDiffEvidenceRequest {
        route,
        viewport,
        revision,
        captured_at_unix_ms,
        mode,
        changed_ratio,
        visual_evidence_ids,
    };

    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/evidence/visual-diff"
        ))
        .bearer_auth(token)
        .json(&metadata)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<VisualDiffEvidenceResponse>()
        .await
        .map_err(err)
}

pub(crate) fn canonical_visual_diff_route(route: &str) -> Result<String, String> {
    let mut route = url::Url::parse(route)
        .map_err(|_| "visual diff route is not a valid URL".to_string())?;
    route.set_query(None);
    route.set_fragment(None);
    Ok(route.to_string())
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

fn trusted_css_dimension(value: f64, axis: &str) -> Result<u32, String> {
    if !value.is_finite()
        || value <= 0.0
        || value > MAX_CSS_VIEWPORT_DIMENSION
    {
        return Err(format!("trusted viewport {axis} is outside the safety range"));
    }

    let rounded = value.round();
    if (value - rounded).abs() > TRUSTED_VIEWPORT_INTEGRAL_TOLERANCE_CSS {
        return Err(format!(
            "trusted viewport {axis} is not an integral CSS dimension"
        ));
    }
    if rounded > u32::MAX as f64 {
        return Err(format!("trusted viewport {axis} exceeds u32::MAX"));
    }

    let dimension = rounded as u32;
    if dimension == 0 {
        return Err(format!("trusted viewport {axis} must be positive"));
    }
    Ok(dimension)
}

fn managed_surface_scale_factor(
    app: &tauri::AppHandle,
    session_id: SessionId,
) -> Result<f64, String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !bridge_surface_label_allowed(window.label(), session_id) {
            return Err("trusted capture preview/session ownership mismatch".into());
        }
        let route_url = window.url().map_err(err)?;
        if !workspace_navigation_allowed(&route_url) {
            return Err("trusted capture refuses a non-loopback managed surface".into());
        }
        let scale_factor = window.scale_factor().map_err(err)?;
        validate_trusted_scale_factor(scale_factor)?;
        return Ok(scale_factor);
    }

    #[cfg(feature = "native-workspace")]
    {
        let label = workspace_surface::workspace_label(session_id);
        if let Some(webview) = app.get_webview(&label) {
            if !bridge_surface_label_allowed(webview.label(), session_id) {
                return Err("trusted capture workspace/session ownership mismatch".into());
            }
            let route_url = webview.url().map_err(err)?;
            if !workspace_navigation_allowed(&route_url) {
                return Err("trusted capture refuses a non-loopback managed surface".into());
            }
            let parent = app
                .get_window("main")
                .ok_or_else(|| "trusted capture main window is unavailable".to_string())?;
            let scale_factor = parent.scale_factor().map_err(err)?;
            validate_trusted_scale_factor(scale_factor)?;
            return Ok(scale_factor);
        }
    }

    Err("trusted capture managed surface is unavailable".into())
}

fn validate_trusted_scale_factor(scale_factor: f64) -> Result<(), String> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 || scale_factor > 8.0 {
        return Err("trusted capture device scale factor is outside the safety range".into());
    }
    Ok(())
}

fn trusted_viewport_from_freeze(
    app: &tauri::AppHandle,
    session_id: SessionId,
    freeze: &FreezeVisualStateReceipt,
) -> Result<ViewportMeta, String> {
    let viewport = ViewportMeta {
        css_width: trusted_css_dimension(freeze.viewport_css_width, "width")?,
        css_height: trusted_css_dimension(freeze.viewport_css_height, "height")?,
        device_scale_factor: managed_surface_scale_factor(app, session_id)?,
    };
    validate_viewport(&viewport)?;
    Ok(viewport)
}

fn validate_trusted_current_viewport(
    app: &tauri::AppHandle,
    session_id: SessionId,
    frame: &CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
    expected_scale_factor: f64,
) -> Result<(), String> {
    let css_width = trusted_css_dimension(freeze.viewport_css_width, "width")?;
    let css_height = trusted_css_dimension(freeze.viewport_css_height, "height")?;
    if frame.viewport.css_width != css_width || frame.viewport.css_height != css_height {
        return Err("trusted current viewport geometry drifted during capture; pixels discarded".into());
    }
    if frame.viewport.device_scale_factor != expected_scale_factor {
        return Err("trusted current viewport scale factor metadata mismatch; pixels discarded".into());
    }
    let current_scale_factor = managed_surface_scale_factor(app, session_id)?;
    if (current_scale_factor - expected_scale_factor).abs() > f64::EPSILON {
        return Err("trusted current viewport device scale factor changed during capture; pixels discarded".into());
    }
    if frame.pixel_width == 0 || frame.pixel_height == 0 {
        return Err("trusted current viewport native pixel dimensions are invalid; pixels discarded".into());
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

pub(crate) async fn wait_for_verification_settle(
    session_id: SessionId,
) -> Result<(), String> {
    wait_for_capture_settle(session_id)
        .await
        .map_err(|_| "trusted Verify settle failed".to_string())
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
        let artifacts = artifacts
            .as_mut()
            .expect("visual artifact store initialized above");
        let retained_resources = &state.retained_resources;

        retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, artifacts.used_bytes())
            .map_err(retained_resource_error)?;
        let projected_bytes = artifacts
            .projected_used_bytes_after_put(&png)
            .map_err(err)?;
        retained_resources
            .admit_projected(RetainedResourceKind::CaptureStorage, projected_bytes)
            .map_err(retained_resource_error)?;

        let put_result = artifacts.put("visual/png", &png).await;
        let actual_bytes = artifacts.used_bytes();
        let reconcile_result = retained_resources
            .synchronize(RetainedResourceKind::CaptureStorage, actual_bytes);

        let artifact = put_result.map_err(err)?;
        reconcile_result.map_err(retained_resource_error)?;
        artifact.id
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

fn retained_resource_error(violation: RetainedResourceViolation) -> String {
    let kind = match violation.kind {
        RetainedResourceKind::CaptureStorage => "capture_storage",
        RetainedResourceKind::Cache => "cache",
    };
    format!(
        "retained resource denied: kind={kind} current={} projected_or_observed={} limit={}",
        violation.current_bytes,
        violation.projected_or_observed_bytes,
        violation.limit_bytes
    )
}

use localview_capture::{
    resolve_progressive_targets, ProgressiveTargetError, ProgressiveTargetKind,
    ProgressiveTargetProvenance,
};

#[derive(Debug, Clone, Serialize)]
pub struct ProgressiveTargetCaptureReceipt {
    pub capture: VisualCaptureReceipt,
    pub level: ProgressiveTargetKind,
    pub provenance: ProgressiveTargetProvenance,
    pub confidence_milli: u16,
    pub snapshot_version: u64,
    pub snapshot_route: String,
}

#[tauri::command]
pub async fn capture_progressive_target(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    reference: ElementRef,
    viewport: ViewportMeta,
    revision: Option<String>,
    level: ProgressiveTargetKind,
) -> Result<ProgressiveTargetCaptureReceipt, String> {
    validate_viewport(&viewport)?;
    preflight_managed_surface(&app, session_id)?;

    let capture_gate = session_capture_gate(&state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;

    let snapshot = fresh_semantic_snapshot(session_id).await?;
    let plan = resolve_progressive_targets(&snapshot, &reference).map_err(progressive_target_error)?;
    if snapshot.viewport != (viewport.css_width, viewport.css_height) {
        return Err("progressive target viewport does not match fresh semantic snapshot".into());
    }

    let resolved = plan
        .targets
        .iter()
        .find(|target| target.kind == level)
        .cloned()
        .ok_or_else(|| "requested progressive target level is unavailable".to_string())?;
    let target = match level {
        ProgressiveTargetKind::Viewport => RequestedCaptureTarget::Viewport,
        _ => RequestedCaptureTarget::Region(resolved.rect.clone()),
    };

    let (frame, freeze) =
        capture_redacted_viewport_after_gate(app, session_id, viewport, revision).await?;
    validate_progressive_live_state(&frame, &freeze, &snapshot)?;
    let frame = apply_capture_target(frame, &freeze, &target)?;
    let capture = persist_and_register(&state, session_id, frame, &target).await?;

    Ok(ProgressiveTargetCaptureReceipt {
        capture,
        level,
        provenance: resolved.provenance,
        confidence_milli: resolved.confidence_milli,
        snapshot_version: plan.snapshot_version,
        snapshot_route: plan.route,
    })
}

async fn fresh_semantic_snapshot(session_id: SessionId) -> Result<PageSnapshot, String> {
    let token = read_token().await?;
    control_client()?
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/semantic-snapshot/fresh"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<PageSnapshot>()
        .await
        .map_err(err)
}

fn progressive_target_error(error: ProgressiveTargetError) -> String {
    match error {
        ProgressiveTargetError::InvalidViewport => {
            "fresh semantic snapshot viewport is invalid".to_string()
        }
        ProgressiveTargetError::ReferenceNotFound => {
            "progressive target reference is unavailable in fresh semantic snapshot".to_string()
        }
        ProgressiveTargetError::InvalidElementGeometry => {
            "progressive target element geometry is unavailable".to_string()
        }
    }
}

fn validate_progressive_live_state(
    frame: &CapturedFrame,
    freeze: &FreezeVisualStateReceipt,
    snapshot: &PageSnapshot,
) -> Result<(), String> {
    let (snapshot_width, snapshot_height) = snapshot.viewport;
    if frame.viewport.css_width != snapshot_width
        || frame.viewport.css_height != snapshot_height
        || freeze.viewport_css_width != snapshot_width as f64
        || freeze.viewport_css_height != snapshot_height as f64
    {
        return Err(
            "progressive target live viewport drifted from fresh semantic snapshot; pixels discarded"
                .into(),
        );
    }

    if progressive_route_signature(&frame.route)? != progressive_route_signature(&snapshot.route)? {
        return Err(
            "progressive target live route drifted from fresh semantic snapshot; pixels discarded"
                .into(),
        );
    }
    Ok(())
}

fn progressive_route_signature(
    route: &str,
) -> Result<(String, Option<String>, Option<u16>, String, Vec<(String, String)>), String> {
    let url = url::Url::parse(route)
        .map_err(|_| "progressive target route is not a valid URL".to_string())?;
    let mut query = Vec::new();
    let mut sensitive_keys = std::collections::BTreeSet::new();
    for (key, value) in url.query_pairs() {
        let key = key.into_owned();
        let lower = key.to_ascii_lowercase();
        let sensitive = lower.contains("token")
            || lower.contains("key")
            || lower.contains("secret")
            || lower.contains("password")
            || lower.contains("authorization");
        if sensitive {
            if sensitive_keys.insert(key.clone()) {
                query.push((key, "[REDACTED]".to_string()));
            }
        } else {
            query.push((key, value.into_owned()));
        }
    }

    Ok((
        url.scheme().to_string(),
        url.host_str().map(ToOwned::to_owned),
        url.port_or_known_default(),
        url.path().to_string(),
        query,
    ))
}

#[cfg(test)]
mod trusted_capture_v21_tests {
    use super::*;

    #[test]
    fn trusted_css_dimension_accepts_integral_geometry() {
        assert_eq!(trusted_css_dimension(1280.0, "width").unwrap(), 1280);
        assert_eq!(trusted_css_dimension(719.995, "height").unwrap(), 720);
    }

    #[test]
    fn trusted_css_dimension_rejects_non_integral_and_invalid_geometry() {
        assert!(trusted_css_dimension(1280.25, "width").is_err());
        assert!(trusted_css_dimension(0.0, "width").is_err());
        assert!(trusted_css_dimension(-1.0, "width").is_err());
        assert!(trusted_css_dimension(f64::NAN, "width").is_err());
        assert!(trusted_css_dimension(f64::INFINITY, "width").is_err());
        assert!(trusted_css_dimension(MAX_CSS_VIEWPORT_DIMENSION + 1.0, "width").is_err());
    }

    #[test]
    fn trusted_scale_factor_is_bounded() {
        assert!(validate_trusted_scale_factor(1.0).is_ok());
        assert!(validate_trusted_scale_factor(2.0).is_ok());
        assert!(validate_trusted_scale_factor(0.0).is_err());
        assert!(validate_trusted_scale_factor(-1.0).is_err());
        assert!(validate_trusted_scale_factor(f64::NAN).is_err());
        assert!(validate_trusted_scale_factor(8.01).is_err());
    }
}

include!("visual_packet_impl.rs");
