#![forbid(unsafe_code)]

use localview_protocol::{ElementRef, PageSnapshot, Rect, SemanticNode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureTarget {
    Viewport,
    FullPage,
    Element { reference: ElementRef },
    Region { rect: Rect },
    Responsive { viewports: Vec<(u32, u32)> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgressiveTargetKind {
    Element,
    Component,
    Section,
    Viewport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressiveTargetProvenance {
    StableElementRef { reference: ElementRef },
    SourceComponent { component: String, owner_ref: ElementRef },
    SemanticSection { owner_ref: ElementRef, boundary: String },
    ViewportFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressiveResolvedTarget {
    pub kind: ProgressiveTargetKind,
    pub rect: Rect,
    pub provenance: ProgressiveTargetProvenance,
    pub confidence_milli: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgressiveTargetPlan {
    pub reference: ElementRef,
    pub snapshot_version: u64,
    pub route: String,
    pub viewport: (u32, u32),
    pub targets: Vec<ProgressiveResolvedTarget>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgressiveTargetError {
    InvalidViewport,
    ReferenceNotFound,
    InvalidElementGeometry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StableCapturePolicy {
    pub wait_dom_ready: bool,
    pub wait_fonts: bool,
    pub wait_images: bool,
    pub wait_hmr_settle: bool,
    pub wait_layout_stable: bool,
    pub network_quiet_ms: Option<u64>,
    pub freeze_animation: bool,
    pub freeze_transition: bool,
    pub mask_selectors: Vec<String>,
    pub timeout_ms: u64,
}

impl Default for StableCapturePolicy {
    fn default() -> Self {
        Self {
            wait_dom_ready: true,
            wait_fonts: true,
            wait_images: true,
            wait_hmr_settle: true,
            wait_layout_stable: true,
            network_quiet_ms: Some(250),
            freeze_animation: true,
            freeze_transition: true,
            mask_selectors: vec![
                "[data-localview-private]".into(),
                "[data-private]".into(),
                "[data-sensitive]".into(),
                "input[type=\"password\"]".into(),
                "input[autocomplete=\"current-password\"]".into(),
                "input[autocomplete=\"new-password\"]".into(),
                "input[autocomplete=\"one-time-code\"]".into(),
            ],
            timeout_ms: 5_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureStage {
    DomReady,
    FontsReady,
    ImagesReady,
    HmrSettled,
    LayoutStable,
    NetworkQuiet,
    AnimationsFrozen,
    Masked,
    Captured,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapturePlan {
    pub target: CaptureTarget,
    pub policy: StableCapturePolicy,
    pub stages: Vec<CaptureStage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettleReason {
    NoSemanticSnapshot,
    DomNotReady,
    FontsPending,
    ImagesPending,
    HmrRecent,
    DomMutationRecent,
    LayoutRecent,
    NetworkRecent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettleObservation {
    pub now_unix_ms: i64,
    pub latest_semantic_at_unix_ms: Option<i64>,
    pub ready_state: Option<String>,
    pub fonts_status: Option<String>,
    pub pending_images: Option<u32>,
    pub latest_hmr_at_unix_ms: Option<i64>,
    pub latest_dom_mutation_at_unix_ms: Option<i64>,
    pub latest_layout_at_unix_ms: Option<i64>,
    pub latest_network_at_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettleDecision {
    pub stable: bool,
    pub reasons: Vec<SettleReason>,
    pub retry_after_ms: u64,
}

const HMR_QUIET_MS: u64 = 300;
const STRUCTURE_QUIET_MS: u64 = 200;
const DEFAULT_RETRY_MS: u64 = 50;

pub fn evaluate_settle(
    policy: &StableCapturePolicy,
    observation: &SettleObservation,
) -> SettleDecision {
    let mut reasons = Vec::new();
    let needs_snapshot = policy.wait_dom_ready || policy.wait_fonts || policy.wait_images;

    if needs_snapshot && observation.latest_semantic_at_unix_ms.is_none() {
        reasons.push(SettleReason::NoSemanticSnapshot);
    } else {
        if policy.wait_dom_ready
            && observation.ready_state.as_deref() != Some("complete")
        {
            reasons.push(SettleReason::DomNotReady);
        }
        if policy.wait_fonts
            && !matches!(
                observation.fonts_status.as_deref(),
                Some("loaded" | "unsupported")
            )
        {
            reasons.push(SettleReason::FontsPending);
        }
        if policy.wait_images && observation.pending_images != Some(0) {
            reasons.push(SettleReason::ImagesPending);
        }
    }

    if policy.wait_hmr_settle
        && recent(
            observation.now_unix_ms,
            observation.latest_hmr_at_unix_ms,
            HMR_QUIET_MS,
        )
    {
        reasons.push(SettleReason::HmrRecent);
    }

    if policy.wait_layout_stable {
        if recent(
            observation.now_unix_ms,
            observation.latest_dom_mutation_at_unix_ms,
            STRUCTURE_QUIET_MS,
        ) {
            reasons.push(SettleReason::DomMutationRecent);
        }
        if recent(
            observation.now_unix_ms,
            observation.latest_layout_at_unix_ms,
            STRUCTURE_QUIET_MS,
        ) {
            reasons.push(SettleReason::LayoutRecent);
        }
    }

    if let Some(quiet_ms) = policy.network_quiet_ms {
        if recent(
            observation.now_unix_ms,
            observation.latest_network_at_unix_ms,
            quiet_ms,
        ) {
            reasons.push(SettleReason::NetworkRecent);
        }
    }

    SettleDecision {
        stable: reasons.is_empty(),
        reasons,
        retry_after_ms: DEFAULT_RETRY_MS.clamp(25, 100),
    }
}

fn recent(now_unix_ms: i64, event_at_unix_ms: Option<i64>, quiet_ms: u64) -> bool {
    let Some(event_at) = event_at_unix_ms else {
        return false;
    };
    if event_at >= now_unix_ms {
        return true;
    }
    now_unix_ms.saturating_sub(event_at) < quiet_ms.min(i64::MAX as u64) as i64
}

pub fn build_plan(target: CaptureTarget, policy: StableCapturePolicy) -> CapturePlan {
    let mut stages = Vec::new();
    if policy.wait_dom_ready {
        stages.push(CaptureStage::DomReady);
    }
    if policy.wait_fonts {
        stages.push(CaptureStage::FontsReady);
    }
    if policy.wait_images {
        stages.push(CaptureStage::ImagesReady);
    }
    if policy.wait_hmr_settle {
        stages.push(CaptureStage::HmrSettled);
    }
    if policy.wait_layout_stable {
        stages.push(CaptureStage::LayoutStable);
    }
    if policy.network_quiet_ms.is_some() {
        stages.push(CaptureStage::NetworkQuiet);
    }
    if policy.freeze_animation || policy.freeze_transition {
        stages.push(CaptureStage::AnimationsFrozen);
    }
    if !policy.mask_selectors.is_empty() {
        stages.push(CaptureStage::Masked);
    }
    stages.push(CaptureStage::Captured);
    stages.push(CaptureStage::Restored);
    CapturePlan {
        target,
        policy,
        stages,
    }
}

pub fn resolve_progressive_targets(
    snapshot: &PageSnapshot,
    reference: &str,
) -> Result<ProgressiveTargetPlan, ProgressiveTargetError> {
    let viewport = snapshot.viewport;
    if viewport.0 == 0 || viewport.1 == 0 {
        return Err(ProgressiveTargetError::InvalidViewport);
    }

    let mut path = Vec::new();
    if !find_semantic_path(&snapshot.root, reference, &mut path) {
        return Err(ProgressiveTargetError::ReferenceNotFound);
    }
    let target = path
        .last()
        .copied()
        .ok_or(ProgressiveTargetError::ReferenceNotFound)?;
    let raw_element = target
        .rect
        .as_ref()
        .ok_or(ProgressiveTargetError::InvalidElementGeometry)?;
    let element_rect = validate_and_clip(raw_element, viewport)
        .ok_or(ProgressiveTargetError::InvalidElementGeometry)?;

    let component = resolve_component_ancestor(&path, &element_rect, viewport);
    let section = resolve_section_ancestor(&path, &element_rect, viewport);

    let mut targets = Vec::with_capacity(4);
    push_unique_target(
        &mut targets,
        ProgressiveResolvedTarget {
            kind: ProgressiveTargetKind::Element,
            rect: expand(&element_rect, 120.0, viewport),
            provenance: ProgressiveTargetProvenance::StableElementRef {
                reference: target.reference.clone(),
            },
            confidence_milli: 1000,
        },
    );

    if let Some((owner, rect, component_name)) = component {
        push_unique_target(
            &mut targets,
            ProgressiveResolvedTarget {
                kind: ProgressiveTargetKind::Component,
                rect,
                provenance: ProgressiveTargetProvenance::SourceComponent {
                    component: component_name,
                    owner_ref: owner.reference.clone(),
                },
                confidence_milli: 950,
            },
        );
    }

    if let Some((owner, rect, boundary)) = section {
        push_unique_target(
            &mut targets,
            ProgressiveResolvedTarget {
                kind: ProgressiveTargetKind::Section,
                rect,
                provenance: ProgressiveTargetProvenance::SemanticSection {
                    owner_ref: owner.reference.clone(),
                    boundary,
                },
                confidence_milli: 850,
            },
        );
    }

    targets.push(ProgressiveResolvedTarget {
        kind: ProgressiveTargetKind::Viewport,
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: viewport.0 as f64,
            height: viewport.1 as f64,
        },
        provenance: ProgressiveTargetProvenance::ViewportFallback,
        confidence_milli: 1000,
    });

    Ok(ProgressiveTargetPlan {
        reference: reference.to_owned(),
        snapshot_version: snapshot.version,
        route: snapshot.route.clone(),
        viewport,
        targets,
    })
}

fn find_semantic_path<'a>(
    node: &'a SemanticNode,
    reference: &str,
    path: &mut Vec<&'a SemanticNode>,
) -> bool {
    path.push(node);
    if node.reference == reference {
        return true;
    }
    for child in &node.children {
        if find_semantic_path(child, reference, path) {
            return true;
        }
    }
    path.pop();
    false
}

fn resolve_component_ancestor<'a>(
    path: &[&'a SemanticNode],
    element: &Rect,
    viewport: (u32, u32),
) -> Option<(&'a SemanticNode, Rect, String)> {
    let target = *path.last()?;
    let component_name = target.source.as_ref()?.component.as_deref()?;
    path[..path.len().saturating_sub(1)]
        .iter()
        .rev()
        .copied()
        .find_map(|ancestor| {
            let ancestor_component = ancestor.source.as_ref()?.component.as_deref()?;
            if ancestor_component != component_name {
                return None;
            }
            let rect = validate_and_clip(ancestor.rect.as_ref()?, viewport)?;
            if !contains_rect(&rect, element) {
                return None;
            }
            Some((ancestor, rect, component_name.to_owned()))
        })
}

fn resolve_section_ancestor<'a>(
    path: &[&'a SemanticNode],
    element: &Rect,
    viewport: (u32, u32),
) -> Option<(&'a SemanticNode, Rect, String)> {
    path[..path.len().saturating_sub(1)]
        .iter()
        .rev()
        .copied()
        .find_map(|ancestor| {
            let boundary = semantic_section_boundary(ancestor)?;
            let rect = validate_and_clip(ancestor.rect.as_ref()?, viewport)?;
            if !contains_rect(&rect, element) {
                return None;
            }
            Some((ancestor, rect, boundary))
        })
}

fn semantic_section_boundary(node: &SemanticNode) -> Option<String> {
    let tag = node.tag.to_ascii_lowercase();
    if matches!(tag.as_str(), "section" | "main" | "article" | "nav" | "aside" | "form") {
        return Some(format!("tag:{tag}"));
    }
    let role = node.role.as_deref()?.to_ascii_lowercase();
    if matches!(
        role.as_str(),
        "region" | "main" | "navigation" | "complementary" | "form"
    ) {
        return Some(format!("role:{role}"));
    }
    None
}

fn validate_and_clip(rect: &Rect, viewport: (u32, u32)) -> Option<Rect> {
    if viewport.0 == 0
        || viewport.1 == 0
        || !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return None;
    }
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    if !right.is_finite() || !bottom.is_finite() {
        return None;
    }

    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let clipped_right = right.min(viewport.0 as f64);
    let clipped_bottom = bottom.min(viewport.1 as f64);
    if clipped_right <= left || clipped_bottom <= top {
        return None;
    }
    Some(Rect {
        x: left,
        y: top,
        width: clipped_right - left,
        height: clipped_bottom - top,
    })
}

fn contains_rect(container: &Rect, child: &Rect) -> bool {
    let container_right = container.x + container.width;
    let container_bottom = container.y + container.height;
    let child_right = child.x + child.width;
    let child_bottom = child.y + child.height;
    container.x <= child.x
        && container.y <= child.y
        && container_right >= child_right
        && container_bottom >= child_bottom
}

fn push_unique_target(
    targets: &mut Vec<ProgressiveResolvedTarget>,
    target: ProgressiveResolvedTarget,
) {
    if targets.iter().any(|existing| existing.rect == target.rect) {
        return;
    }
    targets.push(target);
}

pub fn progressive_regions(
    element: &Rect,
    component: Option<&Rect>,
    section: Option<&Rect>,
    viewport: (u32, u32),
) -> Vec<Rect> {
    let mut out = vec![expand(element, 120.0, viewport)];
    if let Some(rect) = component {
        out.push(clamp(rect, viewport));
    }
    if let Some(rect) = section {
        out.push(clamp(rect, viewport));
    }
    out.push(Rect {
        x: 0.0,
        y: 0.0,
        width: viewport.0 as f64,
        height: viewport.1 as f64,
    });
    out
}

fn expand(rect: &Rect, pad: f64, viewport: (u32, u32)) -> Rect {
    clamp(
        &Rect {
            x: rect.x - pad,
            y: rect.y - pad,
            width: rect.width + pad * 2.0,
            height: rect.height + pad * 2.0,
        },
        viewport,
    )
}

fn clamp(rect: &Rect, viewport: (u32, u32)) -> Rect {
    let x = rect.x.max(0.0).min(viewport.0 as f64);
    let y = rect.y.max(0.0).min(viewport.1 as f64);
    Rect {
        x,
        y,
        width: rect.width.min(viewport.0 as f64 - x).max(0.0),
        height: rect.height.min(viewport.1 as f64 - y).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_restores_after_capture() {
        let plan = build_plan(CaptureTarget::Viewport, Default::default());
        assert_eq!(plan.stages.last(), Some(&CaptureStage::Restored));
    }
}
