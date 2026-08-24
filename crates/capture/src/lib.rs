#![forbid(unsafe_code)]

use localview_protocol::{ElementRef, Rect};
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
