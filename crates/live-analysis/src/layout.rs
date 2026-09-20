use localview_layout::{
    DisplayMode, FlexDirection, FlexWrap, LayoutAnalysis, LayoutElement, LayoutStyleEvidence,
    MAX_LAYOUT_ELEMENTS, OverflowMode, PositionMode, VisibilityEvidence, analyze,
};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_protocol::Rect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_LAYOUT_TREE_DEPTH: usize = 16;
const MAX_REFERENCE_BYTES: usize = 128;
const MAX_STYLE_VALUE_BYTES: usize = 180;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LiveLayoutAnalysis {
    pub snapshot_seq: Option<u64>,
    pub snapshot_version: Option<u64>,
    pub analysis: LayoutAnalysis,
}

pub fn analyze_layout_events(events: &[ObserverEvent]) -> LiveLayoutAnalysis {
    let Some(event) = events
        .iter()
        .filter(|event| event.kind == ObserverEventKind::SemanticSnapshot)
        .max_by_key(|event| event.seq)
    else {
        return LiveLayoutAnalysis::default();
    };

    let packet = event.payload.get("snapshot").unwrap_or(&event.payload);
    let snapshot_version = packet.get("version").and_then(Value::as_u64);
    let viewport = packet
        .get("viewport")
        .and_then(|value| {
            Some((
                value.get("width")?.as_f64()?,
                value.get("height")?.as_f64()?,
            ))
        })
        .unwrap_or((f64::NAN, f64::NAN));

    let mut elements = Vec::new();
    let mut projection_truncated = false;
    if let Some(root) = packet.get("semantic_tree") {
        project_node(root, None, 0, &mut elements, &mut projection_truncated);
    }

    let mut analysis = analyze(&elements, viewport);
    analysis.input_truncated |= projection_truncated;
    LiveLayoutAnalysis {
        snapshot_seq: Some(event.seq),
        snapshot_version,
        analysis,
    }
}

fn project_node(
    node: &Value,
    retained_parent: Option<&str>,
    depth: usize,
    elements: &mut Vec<LayoutElement>,
    truncated: &mut bool,
) {
    if depth > MAX_LAYOUT_TREE_DEPTH {
        *truncated = true;
        return;
    }
    if elements.len() >= MAX_LAYOUT_ELEMENTS {
        *truncated = true;
        return;
    }

    let reference = node
        .get("ref")
        .and_then(Value::as_str)
        .and_then(bounded_ref);
    let rect = node.get("rect").and_then(parse_rect);
    let retained_reference = match (reference, rect) {
        (Some(reference), Some(rect)) => {
            let element = LayoutElement {
                reference: reference.to_owned(),
                rect,
                parent: retained_parent.map(str::to_owned),
                interactive: node
                    .get("interactive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                font_size: node
                    .get("style")
                    .and_then(|style| style.get("fontSize"))
                    .and_then(Value::as_str)
                    .and_then(parse_css_px)
                    .filter(|value| *value > 0.0),
                padding: node.get("style").and_then(parse_padding),
                style: node.get("style").map(parse_style).unwrap_or_default(),
                visibility: node
                    .get("visibility")
                    .map(parse_visibility)
                    .unwrap_or_default(),
            };
            elements.push(element);
            Some(reference)
        }
        _ => None,
    };

    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            if elements.len() >= MAX_LAYOUT_ELEMENTS {
                *truncated = true;
                break;
            }
            project_node(child, retained_reference, depth + 1, elements, truncated);
        }
    }
}

fn parse_rect(value: &Value) -> Option<Rect> {
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    let width = value.get("width")?.as_f64()?;
    let height = value.get("height")?.as_f64()?;
    if !x.is_finite() || !y.is_finite() || !width.is_finite() || !height.is_finite() {
        return None;
    }
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn parse_style(value: &Value) -> LayoutStyleEvidence {
    let mut style = LayoutStyleEvidence::default();
    let Some(object) = value.as_object() else {
        return style;
    };

    style.display = object
        .get("display")
        .and_then(Value::as_str)
        .map(parse_display)
        .unwrap_or_default();
    style.position = object
        .get("position")
        .and_then(Value::as_str)
        .and_then(parse_position);
    style.overflow_x = object
        .get("overflowX")
        .and_then(Value::as_str)
        .and_then(parse_overflow);
    style.overflow_y = object
        .get("overflowY")
        .and_then(Value::as_str)
        .and_then(parse_overflow);
    style.flex_direction = object
        .get("flexDirection")
        .and_then(Value::as_str)
        .and_then(parse_flex_direction);
    style.flex_wrap = object
        .get("flexWrap")
        .and_then(Value::as_str)
        .and_then(parse_flex_wrap);
    style.justify_content = object
        .get("justifyContent")
        .and_then(Value::as_str)
        .and_then(bounded_style_text)
        .map(str::to_owned);
    style.align_items = object
        .get("alignItems")
        .and_then(Value::as_str)
        .and_then(bounded_style_text)
        .map(str::to_owned);
    style.row_gap = object
        .get("rowGap")
        .and_then(Value::as_str)
        .and_then(parse_css_px);
    style.column_gap = object
        .get("columnGap")
        .and_then(Value::as_str)
        .and_then(parse_css_px);
    let generic_gap = object
        .get("gap")
        .and_then(Value::as_str)
        .and_then(parse_css_px);
    if style.row_gap.is_none() {
        style.row_gap = generic_gap;
    }
    if style.column_gap.is_none() {
        style.column_gap = generic_gap;
    }
    style.grid_template_columns = object
        .get("gridTemplateColumns")
        .and_then(Value::as_str)
        .and_then(bounded_style_text)
        .map(str::to_owned);
    style.grid_template_rows = object
        .get("gridTemplateRows")
        .and_then(Value::as_str)
        .and_then(bounded_style_text)
        .map(str::to_owned);
    style.z_index = object
        .get("zIndex")
        .and_then(Value::as_str)
        .and_then(parse_z_index);
    style
}

fn parse_visibility(value: &Value) -> VisibilityEvidence {
    VisibilityEvidence {
        in_viewport: value.get("inViewport").and_then(Value::as_bool),
        clipped: value.get("clipped").and_then(Value::as_bool),
        occluded: value.get("occluded").and_then(Value::as_bool),
        occluded_by: value
            .get("occludedBy")
            .and_then(Value::as_str)
            .and_then(bounded_ref)
            .map(str::to_owned),
        sampled: value
            .get("sampled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_padding(value: &Value) -> Option<[f64; 4]> {
    let object = value.as_object()?;
    let top = object.get("paddingTop")?.as_str().and_then(parse_css_px)?;
    let right = object
        .get("paddingRight")?
        .as_str()
        .and_then(parse_css_px)?;
    let bottom = object
        .get("paddingBottom")?
        .as_str()
        .and_then(parse_css_px)?;
    let left = object.get("paddingLeft")?.as_str().and_then(parse_css_px)?;
    if [top, right, bottom, left]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }
    Some([top, right, bottom, left])
}

fn parse_display(value: &str) -> DisplayMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "block" => DisplayMode::Block,
        "inline" => DisplayMode::Inline,
        "flex" => DisplayMode::Flex,
        "inline-flex" => DisplayMode::InlineFlex,
        "grid" => DisplayMode::Grid,
        "inline-grid" => DisplayMode::InlineGrid,
        _ => DisplayMode::Other,
    }
}

fn parse_flex_direction(value: &str) -> Option<FlexDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        "row" => Some(FlexDirection::Row),
        "row-reverse" => Some(FlexDirection::RowReverse),
        "column" => Some(FlexDirection::Column),
        "column-reverse" => Some(FlexDirection::ColumnReverse),
        _ => None,
    }
}

fn parse_flex_wrap(value: &str) -> Option<FlexWrap> {
    match value.trim().to_ascii_lowercase().as_str() {
        "nowrap" => Some(FlexWrap::NoWrap),
        "wrap" => Some(FlexWrap::Wrap),
        "wrap-reverse" => Some(FlexWrap::WrapReverse),
        _ => None,
    }
}

fn parse_overflow(value: &str) -> Option<OverflowMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "visible" => Some(OverflowMode::Visible),
        "hidden" => Some(OverflowMode::Hidden),
        "clip" => Some(OverflowMode::Clip),
        "auto" => Some(OverflowMode::Auto),
        "scroll" => Some(OverflowMode::Scroll),
        _ => None,
    }
}

fn parse_position(value: &str) -> Option<PositionMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "static" => Some(PositionMode::Static),
        "relative" => Some(PositionMode::Relative),
        "absolute" => Some(PositionMode::Absolute),
        "fixed" => Some(PositionMode::Fixed),
        "sticky" => Some(PositionMode::Sticky),
        _ => None,
    }
}

fn parse_css_px(value: &str) -> Option<f64> {
    let value = value.trim();
    let numeric = value.strip_suffix("px")?;
    let parsed = numeric.trim().parse::<f64>().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn parse_z_index(value: &str) -> Option<i32> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("auto") || value.is_empty() {
        return None;
    }
    value.parse::<i32>().ok()
}

fn bounded_ref(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_REFERENCE_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    Some(value)
}

fn bounded_style_text(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_STYLE_VALUE_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    Some(value)
}
