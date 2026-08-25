use std::time::Instant;

use localview_capture::ProgressiveTargetPlan;
use localview_protocol::DetailLevel;
use localview_token_budget::{
    approximate_tokens, evaluate_perception_budget, select_visual_packet, serialize_with_budget,
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetDecision,
    PerceptionBudgetUsage, SelectedVisualEvidence, VisualPacketCandidate, VisualPacketSelection,
    VisualPacketSelectionMode, VisualPacketSource,
};

#[derive(Debug, Clone, Serialize)]
pub struct VisualPacketCaptureReceipt {
    pub mode: VisualPacketSelectionMode,
    pub changed_ratio: f64,
    pub selection: VisualPacketSelection,
    pub packet: serde_json::Value,
    pub receipts: Vec<VisualCaptureReceipt>,
    pub baseline_cached: bool,
    pub capture_performed: bool,
    pub snapshot_version: Option<u64>,
    pub snapshot_route: Option<String>,
    pub budget_decision: PerceptionBudgetDecision,
}

#[derive(Debug, Serialize)]
struct VisualPacketMetadata<'a> {
    route: &'a str,
    viewport: (u32, u32),
    changed_ratio: f64,
    selection: &'a VisualPacketSelection,
    snapshot_version: Option<u64>,
    snapshot_route: Option<&'a str>,
}

#[tauri::command]
pub async fn capture_visual_packet(
    app: tauri::AppHandle,
    state: tauri::State<'_, VisualCaptureState>,
    session_id: SessionId,
    reference: Option<ElementRef>,
    viewport: ViewportMeta,
    revision: Option<String>,
    budget: PerceptionBudgetContract,
    budget_escalation_reason: Option<BudgetEscalationReason>,
) -> Result<VisualPacketCaptureReceipt, String> {
    let started_at = Instant::now();
    validate_viewport(&viewport)?;
    let visual_budget = budget.visual_packet_budget(DetailLevel::Normal);

    if budget.image_regions == 0 {
        let selection = VisualPacketSelection {
            mode: VisualPacketSelectionMode::MetadataOnly,
            selected: Vec::new(),
            dropped_candidates: 0,
        };
        let packet = serialize_with_budget(
            &serde_json::json!({
                "route": serde_json::Value::Null,
                "viewport": [viewport.css_width, viewport.css_height],
                "selection": selection,
                "capture_performed": false
            }),
            &visual_budget.text,
        );
        let usage = PerceptionBudgetUsage {
            latency_ms: elapsed_ms(started_at),
            text_tokens: visual_packet_text_tokens(&packet),
            image_regions: selection.selected.len(),
            chromium_spawns: 0,
        };
        let budget_decision = evaluate_perception_budget(
            &budget,
            &usage,
            budget_escalation_reason,
        )
        .map_err(perception_budget_violation)?;
        return Ok(VisualPacketCaptureReceipt {
            mode: VisualPacketSelectionMode::MetadataOnly,
            changed_ratio: 0.0,
            selection,
            packet,
            receipts: Vec::new(),
            baseline_cached: false,
            capture_performed: false,
            snapshot_version: None,
            snapshot_route: None,
            budget_decision,
        });
    }

    preflight_managed_surface(&app, session_id)?;
    let capture_gate = session_capture_gate(&state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;

    let progressive = match reference.as_ref() {
        Some(reference) => {
            let snapshot = fresh_semantic_snapshot(session_id).await?;
            let plan = resolve_progressive_targets(&snapshot, reference)
                .map_err(progressive_target_error)?;
            if snapshot.viewport != (viewport.css_width, viewport.css_height) {
                return Err("visual packet viewport does not match fresh semantic snapshot".into());
            }
            Some((snapshot, plan))
        }
        None => None,
    };

    let (frame, freeze) =
        capture_redacted_viewport_after_gate(app, session_id, viewport, revision).await?;
    if let Some((snapshot, _)) = progressive.as_ref() {
        validate_progressive_live_state(&frame, &freeze, snapshot)?;
    } else {
        validate_changed_viewport(&frame, &freeze)?;
    }

    let image = Arc::new(
        decode_png_rgba(&frame.png)
            .map_err(|_| "visual packet decode failed; pixels discarded".to_string())?,
    );
    if (image.width, image.height) != (frame.pixel_width, frame.pixel_height) {
        return Err("visual packet native pixel metadata mismatch; pixels discarded".into());
    }

    let context = changed_baseline_context(&frame);
    let baseline = compatible_changed_baseline(&state, session_id, &context).await?;
    let changed_plan = match baseline.as_deref() {
        Some(before) => plan_changed_css_regions(
            before,
            image.as_ref(),
            (freeze.viewport_css_width, freeze.viewport_css_height),
            ChangedRegionPolicy::default(),
        )
        .map_err(|_| "visual packet changed-region planning failed; pixels discarded".to_string())?,
        None => ChangedRegionPlan::Viewport {
            changed_ratio: 1.0,
        },
    };

    let progressive_plan = progressive.as_ref().map(|(_, plan)| plan);
    let candidates = visual_packet_candidates(
        &changed_plan,
        progressive_plan,
        (frame.viewport.css_width, frame.viewport.css_height),
    );
    let selection = select_visual_packet(
        (frame.viewport.css_width, frame.viewport.css_height),
        &candidates,
        &visual_budget,
    )
    .map_err(|error| error.to_string())?;
    let changed_ratio = changed_plan_ratio(&changed_plan);

    let snapshot_version = progressive.as_ref().map(|(snapshot, _)| snapshot.version);
    let snapshot_route = progressive
        .as_ref()
        .map(|(snapshot, _)| snapshot.route.clone());
    let metadata = VisualPacketMetadata {
        route: &frame.route,
        viewport: (frame.viewport.css_width, frame.viewport.css_height),
        changed_ratio,
        selection: &selection,
        snapshot_version,
        snapshot_route: snapshot_route.as_deref(),
    };
    let packet = serialize_with_budget(&metadata, &visual_budget.text);
    let usage = PerceptionBudgetUsage {
        latency_ms: elapsed_ms(started_at),
        text_tokens: visual_packet_text_tokens(&packet),
        image_regions: selection.selected.len(),
        chromium_spawns: 0,
    };
    let budget_decision = evaluate_perception_budget(
        &budget,
        &usage,
        budget_escalation_reason,
    )
    .map_err(perception_budget_violation)?;

    let receipts = persist_visual_packet_selection(
        &state,
        session_id,
        &frame,
        image.as_ref(),
        &freeze,
        &selection,
    )
    .await?;
    let baseline_cached = commit_changed_baseline(&state, session_id, context, image).await?;

    Ok(VisualPacketCaptureReceipt {
        mode: selection.mode,
        changed_ratio,
        selection,
        packet,
        receipts,
        baseline_cached,
        capture_performed: true,
        snapshot_version,
        snapshot_route,
        budget_decision,
    })
}

fn elapsed_ms(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn visual_packet_text_tokens(packet: &serde_json::Value) -> usize {
    approximate_tokens(&serde_json::to_string(packet).unwrap_or_default())
}

fn perception_budget_violation(
    violation: localview_token_budget::PerceptionBudgetViolation,
) -> String {
    format!("{}: {:?}", violation, violation.exceeded)
}

fn changed_plan_ratio(plan: &ChangedRegionPlan) -> f64 {
    match plan {
        ChangedRegionPlan::Unchanged => 0.0,
        ChangedRegionPlan::Regions { changed_ratio, .. }
        | ChangedRegionPlan::Viewport { changed_ratio } => *changed_ratio,
    }
}

fn visual_packet_candidates(
    changed_plan: &ChangedRegionPlan,
    progressive: Option<&ProgressiveTargetPlan>,
    viewport: (u32, u32),
) -> Vec<VisualPacketCandidate> {
    let mut candidates = Vec::new();
    let focus = progressive.and_then(|plan| {
        plan.targets
            .iter()
            .find(|target| target.kind == ProgressiveTargetKind::Element)
            .map(|target| &target.rect)
    });

    match changed_plan {
        ChangedRegionPlan::Unchanged => {}
        ChangedRegionPlan::Regions { regions, .. } => {
            for rect in regions {
                candidates.push(VisualPacketCandidate {
                    source: VisualPacketSource::ChangedRegion,
                    rect: rect.clone(),
                    information_gain_milli: 1000,
                    confidence_milli: 1000,
                    relevance_milli: changed_region_relevance(rect, focus),
                });
            }
        }
        ChangedRegionPlan::Viewport { .. } => {
            candidates.push(VisualPacketCandidate {
                source: VisualPacketSource::ViewportFallback,
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.0 as f64,
                    height: viewport.1 as f64,
                },
                information_gain_milli: 700,
                confidence_milli: 1000,
                relevance_milli: if progressive.is_some() { 550 } else { 700 },
            });
        }
    }

    if let Some(plan) = progressive {
        for target in &plan.targets {
            let (source, information_gain_milli, relevance_milli) = match target.kind {
                ProgressiveTargetKind::Element => {
                    (VisualPacketSource::ProgressiveElement, 900, 1000)
                }
                ProgressiveTargetKind::Component => {
                    (VisualPacketSource::ProgressiveComponent, 800, 925)
                }
                ProgressiveTargetKind::Section => {
                    (VisualPacketSource::ProgressiveSection, 675, 800)
                }
                ProgressiveTargetKind::Viewport => {
                    (VisualPacketSource::ViewportFallback, 450, 550)
                }
            };
            candidates.push(VisualPacketCandidate {
                source,
                rect: target.rect.clone(),
                information_gain_milli,
                confidence_milli: target.confidence_milli,
                relevance_milli,
            });
        }
    }

    candidates
}

fn changed_region_relevance(rect: &Rect, focus: Option<&Rect>) -> u16 {
    let Some(focus) = focus else {
        return 1000;
    };
    if rects_intersect(rect, focus) {
        1000
    } else {
        650
    }
}

fn rects_intersect(left: &Rect, right: &Rect) -> bool {
    left.x < right.x + right.width
        && right.x < left.x + left.width
        && left.y < right.y + right.height
        && right.y < left.y + left.height
}

async fn persist_visual_packet_selection(
    state: &VisualCaptureState,
    session_id: SessionId,
    frame: &CapturedFrame,
    image: &RgbaImage,
    freeze: &FreezeVisualStateReceipt,
    selection: &VisualPacketSelection,
) -> Result<Vec<VisualCaptureReceipt>, String> {
    let mut receipts = Vec::with_capacity(selection.selected.len());
    for selected in &selection.selected {
        let target = visual_packet_requested_target(selected);
        let (png, pixel_width, pixel_height) = match &target {
            RequestedCaptureTarget::Viewport => {
                let png = encode_png_rgba(image)
                    .map_err(|_| "visual packet PNG encode failed; pixels discarded".to_string())?;
                (png, image.width, image.height)
            }
            RequestedCaptureTarget::Region(rect) => {
                let cropped = image
                    .crop_css_rect(
                        (freeze.viewport_css_width, freeze.viewport_css_height),
                        rect,
                    )
                    .map_err(|_| "visual packet crop failed; pixels discarded".to_string())?;
                let png = encode_png_rgba(&cropped)
                    .map_err(|_| "visual packet PNG encode failed; pixels discarded".to_string())?;
                (png, cropped.width, cropped.height)
            }
        };
        let selected_frame = CapturedFrame {
            png,
            pixel_width,
            pixel_height,
            backend: frame.backend,
            viewport: frame.viewport.clone(),
            route: frame.route.clone(),
            revision: frame.revision.clone(),
            captured_at_unix_ms: frame.captured_at_unix_ms,
        };
        receipts.push(persist_and_register(state, session_id, selected_frame, &target).await?);
    }
    Ok(receipts)
}

fn visual_packet_requested_target(selected: &SelectedVisualEvidence) -> RequestedCaptureTarget {
    match selected.source {
        VisualPacketSource::ViewportFallback => RequestedCaptureTarget::Viewport,
        VisualPacketSource::ChangedRegion
        | VisualPacketSource::ProgressiveElement
        | VisualPacketSource::ProgressiveComponent
        | VisualPacketSource::ProgressiveSection => {
            RequestedCaptureTarget::Region(selected.rect.clone())
        }
    }
}
