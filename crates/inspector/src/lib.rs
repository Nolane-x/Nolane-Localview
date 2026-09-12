#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use localview_protocol::{ElementRef, Rect, SourceLocation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Measurement {
    pub from: ElementRef,
    pub to: ElementRef,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
    pub overlap_area: f64,
}

pub fn measure(from: &ElementRef, a: &Rect, to: &ElementRef, b: &Rect) -> Measurement {
    let horizontal_gap = if a.x + a.width < b.x { b.x - (a.x + a.width) } else if b.x + b.width < a.x { a.x - (b.x + b.width) } else { 0.0 };
    let vertical_gap = if a.y + a.height < b.y { b.y - (a.y + a.height) } else if b.y + b.height < a.y { a.y - (b.y + b.height) } else { 0.0 };
    let overlap_width = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let overlap_height = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    Measurement { from: from.clone(), to: to.clone(), horizontal_gap, vertical_gap, overlap_area: overlap_width.max(0.0) * overlap_height.max(0.0) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenshotAnnotation {
    pub id: String,
    pub rect: Rect,
    pub label: String,
    pub reference: Option<ElementRef>,
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayerNode {
    pub reference: ElementRef,
    pub rect: Rect,
    pub z_index: Option<i32>,
    pub stacking_context: bool,
    pub clipped: bool,
    pub opacity: f32,
    pub pointer_events: bool,
    pub children: Vec<LayerNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LayoutModel { Block, Flex, Grid, Absolute, Fixed, Sticky, Unknown }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlexInspection {
    pub direction: String,
    pub wrap: bool,
    pub justify_content: String,
    pub align_items: String,
    pub gap: f64,
    pub item_grow: BTreeMap<ElementRef, f64>,
    pub item_shrink: BTreeMap<ElementRef, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GridInspection {
    pub columns: Vec<String>,
    pub rows: Vec<String>,
    pub column_gap: f64,
    pub row_gap: f64,
    pub areas: Vec<String>,
    pub item_areas: BTreeMap<ElementRef, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutInspection {
    pub reference: ElementRef,
    pub model: LayoutModel,
    pub flex: Option<FlexInspection>,
    pub grid: Option<GridInspection>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CssOrigin { UserAgent, Inherited, Stylesheet, Module, Inline, Runtime }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CssCause {
    pub property: String,
    pub value: String,
    pub selector: Option<String>,
    pub origin: CssOrigin,
    pub specificity: (u16, u16, u16),
    pub important: bool,
    pub source: Option<SourceLocation>,
    pub active: bool,
}

pub fn winning_css_cause<'a>(property: &str, causes: &'a [CssCause]) -> Option<&'a CssCause> {
    causes.iter().filter(|cause| cause.property == property && cause.active).max_by(|left, right| {
        left.important.cmp(&right.important)
            .then_with(|| left.origin.cmp(&right.origin))
            .then_with(|| left.specificity.cmp(&right.specificity))
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind { Dom, ShadowDomOpen, ShadowDomClosed, SameOriginIframe, CrossOriginIframe, Canvas2d, WebGl, PwaShell }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceCapability {
    pub kind: SurfaceKind,
    pub semantic_access: bool,
    pub screenshot_access: bool,
    pub interaction_access: bool,
    pub source_mapping: bool,
    pub limitation: Option<String>,
}

pub fn default_capability(kind: SurfaceKind) -> SurfaceCapability {
    match kind {
        SurfaceKind::Dom => SurfaceCapability { kind, semantic_access: true, screenshot_access: true, interaction_access: true, source_mapping: true, limitation: None },
        SurfaceKind::ShadowDomOpen => SurfaceCapability { kind, semantic_access: true, screenshot_access: true, interaction_access: true, source_mapping: false, limitation: Some("source mapping depends on framework adapter".into()) },
        SurfaceKind::ShadowDomClosed => SurfaceCapability { kind, semantic_access: false, screenshot_access: true, interaction_access: false, source_mapping: false, limitation: Some("closed shadow root is treated as an opaque visual region".into()) },
        SurfaceKind::SameOriginIframe => SurfaceCapability { kind, semantic_access: true, screenshot_access: true, interaction_access: true, source_mapping: false, limitation: Some("iframe maintains an isolated document identity".into()) },
        SurfaceKind::CrossOriginIframe => SurfaceCapability { kind, semantic_access: false, screenshot_access: true, interaction_access: false, source_mapping: false, limitation: Some("cross-origin policy prevents DOM inspection".into()) },
        SurfaceKind::Canvas2d | SurfaceKind::WebGl => SurfaceCapability { kind, semantic_access: false, screenshot_access: true, interaction_access: true, source_mapping: false, limitation: Some("visual-first analysis; DOM semantics are unavailable".into()) },
        SurfaceKind::PwaShell => SurfaceCapability { kind, semantic_access: true, screenshot_access: true, interaction_access: true, source_mapping: true, limitation: Some("service-worker cache state must be tracked separately".into()) },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DebugSnapshot {
    pub route: String,
    pub viewport: (u32, u32),
    pub focused_ref: Option<ElementRef>,
    pub selected_ref: Option<ElementRef>,
    pub layer_tree: Vec<LayerNode>,
    pub css_causes: BTreeMap<ElementRef, Vec<CssCause>>,
    pub surfaces: Vec<SurfaceCapability>,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_reports_real_overlap() {
        let a = Rect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 };
        let b = Rect { x: 50.0, y: 50.0, width: 100.0, height: 100.0 };
        assert_eq!(measure(&"a".into(), &a, &"b".into(), &b).overlap_area, 2500.0);
    }

    #[test]
    fn important_css_wins_over_more_specific_non_important_rule() {
        let causes = vec![
            CssCause { property: "color".into(), value: "red".into(), selector: Some("#id".into()), origin: CssOrigin::Stylesheet, specificity: (1, 0, 0), important: false, source: None, active: true },
            CssCause { property: "color".into(), value: "blue".into(), selector: Some(".class".into()), origin: CssOrigin::Stylesheet, specificity: (0, 1, 0), important: true, source: None, active: true },
        ];
        assert_eq!(winning_css_cause("color", &causes).map(|cause| cause.value.as_str()), Some("blue"));
    }
}