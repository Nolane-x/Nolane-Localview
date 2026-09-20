use localview_protocol::{ElementRef, Rect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayoutIssueClass {
    Deterministic,
    #[default]
    Heuristic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Block,
    Inline,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverflowMode {
    Visible,
    Hidden,
    Clip,
    Auto,
    Scroll,
}

impl OverflowMode {
    pub const fn clips(self) -> bool {
        matches!(self, Self::Hidden | Self::Clip)
    }

    pub const fn scrolls(self) -> bool {
        matches!(self, Self::Auto | Self::Scroll)
    }

    pub const fn constrains(self) -> bool {
        self.clips() || self.scrolls()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PositionMode {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LayoutStyleEvidence {
    pub display: DisplayMode,
    pub flex_direction: Option<FlexDirection>,
    pub flex_wrap: Option<FlexWrap>,
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub row_gap: Option<f64>,
    pub column_gap: Option<f64>,
    pub grid_template_columns: Option<String>,
    pub grid_template_rows: Option<String>,
    pub overflow_x: Option<OverflowMode>,
    pub overflow_y: Option<OverflowMode>,
    pub position: Option<PositionMode>,
    pub z_index: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VisibilityEvidence {
    pub in_viewport: Option<bool>,
    pub clipped: Option<bool>,
    pub occluded: Option<bool>,
    pub occluded_by: Option<ElementRef>,
    pub sampled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutElement {
    pub reference: ElementRef,
    pub rect: Rect,
    pub parent: Option<ElementRef>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub font_size: Option<f64>,
    #[serde(default)]
    pub padding: Option<[f64; 4]>,
    #[serde(default)]
    pub style: LayoutStyleEvidence,
    #[serde(default)]
    pub visibility: VisibilityEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutIssue {
    pub code: String,
    pub severity: Severity,
    pub confidence: f32,
    #[serde(default)]
    pub class: LayoutIssueClass,
    pub refs: Vec<ElementRef>,
    pub message: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutFact {
    pub code: String,
    pub refs: Vec<ElementRef>,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpacingSource {
    SiblingGap,
    Padding,
    EdgeDistance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpacingFamily {
    pub value: f64,
    pub occurrences: usize,
    pub sources: Vec<SpacingSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LayoutAnalysis {
    pub issues: Vec<LayoutIssue>,
    pub facts: Vec<LayoutFact>,
    pub spacing_families: Vec<SpacingFamily>,
    pub analyzed_nodes: usize,
    pub input_truncated: bool,
}
