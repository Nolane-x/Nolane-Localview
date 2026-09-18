from pathlib import Path

def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)

path = Path("apps/desktop/src-tauri/src/visual_capture.rs")
text = path.read_text()

text = replace_once(
    text,
    '''use localview_visual::{
    decode_png_rgba, encode_png_rgba, plan_changed_css_regions, ChangedRegionPlan,
    ChangedRegionPolicy, RgbaImage, VisualBaselineCache, VisualBaselineContext,
};''',
    '''use localview_visual::{
    decode_png_rgba, encode_png_rgba, plan_changed_css_regions, plan_full_page,
    project_full_page_output, stitch_full_page_tile, ChangedRegionPlan, ChangedRegionPolicy,
    FullPageError, FullPagePlan, FullPagePolicy, RgbaImage, VisualBaselineCache,
    VisualBaselineContext,
};''',
    "full-page visual imports",
)

text = replace_once(
    text,
    '''const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;''',
    '''const VISUAL_FREEZE_LEASE_MS: u64 = 8_000;
const FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;
const FULL_PAGE_TRANSACTION_TIMEOUT_MS: u64 = 30_000;
const MAX_PAUSED_ANIMATIONS: u64 = 2_048;
const MAX_POSITIONAL_SCAN_ELEMENTS: u64 = 4_096;''',
    "full-page desktop constants",
)

type_marker = '''#[tauri::command]
pub async fn capture_viewport(
'''
types_and_impl = r'''#[derive(Debug, Clone, Serialize)]
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

    tokio::time::timeout_at(deadline, wait_for_capture_settle(session_id))
        .await
        .map_err(|_| "full_page_transaction_timeout".to_string())?
        .map_err(|_| "full_page_settle_failed".to_string())?;

    let freeze = tokio::time::timeout_at(deadline, freeze_full_page_visual_state(session_id))
        .await
        .map_err(|_| "full_page_transaction_timeout".to_string())?
        .map_err(|_| "full_page_visual_freeze_failed".to_string())?;

    let work = tokio::time::timeout_at(deadline, async {
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
        cleanup_full_page_state(session_id, &viewport, &freeze).await;

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

    persist_full_page_and_register(
        state,
        session_id,
        png,
        &transaction.frame,
        &transaction.plan,
    )
    .await
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
) -> Result<(), String> {
    let scroll_restore = match capture_scroll_to(session_id, &freeze.token, freeze.scroll_y).await {
        Ok(receipt) => {
            validate_capture_scroll_receipt(&receipt, freeze, viewport, freeze.scroll_y).is_ok()
        }
        Err(_) => false,
    };

    let visual_restore = restore_visual_state(session_id, &freeze.token).await.is_ok();

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

fn managed_surface_canonical_route(
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

'''
if text.count(type_marker) != 1:
    raise SystemExit("capture_viewport insertion marker mismatch")
text = text.replace(type_marker, types_and_impl + type_marker, 1)
path.write_text(text)

lib_path = Path("apps/desktop/src-tauri/src/lib.rs")
lib = lib_path.read_text()
needle = "            visual_capture::capture_viewport,\n"
if lib.count(needle) != 1:
    raise SystemExit("Tauri visual capture handler marker mismatch")
lib = lib.replace(
    needle,
    "            visual_capture::capture_full_page,\n" + needle,
    1,
)
lib_path.write_text(lib)
