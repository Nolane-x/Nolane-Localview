use std::collections::BTreeMap;

use localview_design_grammar::{
    DesignEvidenceSamples, DesignMetricSample, EvidenceProvenance, EvidenceStatus,
    ExtractionPolicy, ProjectDesignGrammar, extract_project_grammar,
};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_protocol::Rect;
use localview_quality::{
    CriticNode, CriticSourceHint, DesignGrammarBaseline, SourceHintAuthority,
    VisualCriticReport, analyze_visual_critic, build_design_baseline,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_CRITIC_TREE_DEPTH: usize = 16;
const MAX_CRITIC_NODES: usize = 512;
const MAX_REFERENCE_BYTES: usize = 128;
const MAX_SOURCE_FILE_BYTES: usize = 320;
const MAX_ROUTE_BYTES: usize = 512;
const MAX_STYLE_SAMPLES_PER_NODE: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LiveVisualCriticAnalysis {
    pub snapshot_seq: Option<u64>,
    pub snapshot_version: Option<u64>,
    pub grammar: ProjectDesignGrammar,
    pub critic: VisualCriticReport,
    pub baseline: Option<DesignGrammarBaseline>,
    pub availability_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct ProjectedNode {
    critic: CriticNode,
    parent: Option<String>,
    padding: Vec<f64>,
    gaps: Vec<f64>,
    radius: Vec<f64>,
}

pub fn analyze_visual_critic_events(events: &[ObserverEvent]) -> LiveVisualCriticAnalysis {
    let Some(event) = events
        .iter()
        .filter(|event| event.kind == ObserverEventKind::SemanticSnapshot)
        .max_by_key(|event| event.seq)
    else {
        return unavailable(None, None, "semantic_snapshot_unavailable");
    };

    let packet = event.payload.get("snapshot").unwrap_or(&event.payload);
    let snapshot_version = packet.get("version").and_then(Value::as_u64);
    let Some(viewport) = packet.get("viewport").and_then(parse_viewport) else {
        return unavailable(
            Some(event.seq),
            snapshot_version,
            "viewport_evidence_unavailable",
        );
    };

    let route = event
        .route
        .as_deref()
        .and_then(bounded_route)
        .map(str::to_owned)
        .or_else(|| {
            packet
                .get("route")
                .and_then(Value::as_str)
                .and_then(bounded_route)
                .map(str::to_owned)
        });
    let provenance = EvidenceProvenance {
        snapshot_seq: Some(event.seq),
        snapshot_version,
        route,
        viewport_width: f64_to_u32(viewport.0),
        viewport_height: f64_to_u32(viewport.1),
        evidence_keys: vec!["live_semantic_snapshot".into()],
    };

    let mut projected = Vec::new();
    let mut truncated = false;
    if let Some(root) = packet.get("semantic_tree") {
        project_node(root, None, 0, &mut projected, &mut truncated);
    }
    if projected.is_empty() {
        return unavailable(
            Some(event.seq),
            snapshot_version,
            "semantic_geometry_unavailable",
        );
    }

    let samples = build_samples(&projected, provenance, truncated);
    let grammar = extract_project_grammar(&samples, ExtractionPolicy::default());
    let critic_nodes = projected
        .iter()
        .map(|node| node.critic.clone())
        .collect::<Vec<_>>();
    let critic = analyze_visual_critic(&critic_nodes, viewport, &grammar);
    let baseline = Some(build_design_baseline(grammar.clone(), &critic));

    LiveVisualCriticAnalysis {
        snapshot_seq: Some(event.seq),
        snapshot_version,
        grammar,
        critic,
        baseline,
        availability_reason: None,
    }
}

fn unavailable(
    snapshot_seq: Option<u64>,
    snapshot_version: Option<u64>,
    reason: &str,
) -> LiveVisualCriticAnalysis {
    LiveVisualCriticAnalysis {
        snapshot_seq,
        snapshot_version,
        availability_reason: Some(reason.into()),
        ..Default::default()
    }
}

fn project_node(
    node: &Value,
    retained_parent: Option<&str>,
    depth: usize,
    projected: &mut Vec<ProjectedNode>,
    truncated: &mut bool,
) {
    if depth > MAX_CRITIC_TREE_DEPTH || projected.len() >= MAX_CRITIC_NODES {
        *truncated = true;
        return;
    }

    let reference = node
        .get("ref")
        .and_then(Value::as_str)
        .and_then(bounded_ref);
    let rect = node.get("rect").and_then(parse_rect);
    let retained_reference = match (reference, rect) {
        (Some(reference), Some(rect)) if rect.width > 0.0 && rect.height > 0.0 => {
            let style = node.get("style").and_then(Value::as_object);
            projected.push(ProjectedNode {
                critic: CriticNode {
                    reference: reference.to_owned(),
                    rect,
                    interactive: node
                        .get("interactive")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    font_size: style.and_then(|style| style_px(style, "fontSize")),
                    font_weight: style.and_then(|style| numeric_weight(style, "fontWeight")),
                    line_height: style.and_then(|style| style_px(style, "lineHeight")),
                    contrast: None,
                    source_hint: parse_source_hint(
                        node.get("sourceHint").or_else(|| node.get("source")),
                    ),
                },
                parent: retained_parent.map(str::to_owned),
                padding: style.map(parse_padding).unwrap_or_default(),
                gaps: style.map(parse_gaps).unwrap_or_default(),
                radius: style.map(parse_radius).unwrap_or_default(),
            });
            Some(reference)
        }
        _ => None,
    };

    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            if projected.len() >= MAX_CRITIC_NODES {
                *truncated = true;
                break;
            }
            project_node(
                child,
                retained_reference,
                depth + 1,
                projected,
                truncated,
            );
        }
    }
}

fn build_samples(
    projected: &[ProjectedNode],
    provenance: EvidenceProvenance,
    input_truncated: bool,
) -> DesignEvidenceSamples {
    let mut samples = DesignEvidenceSamples {
        observed_nodes: projected.len(),
        input_truncated,
        ..Default::default()
    };

    for node in projected {
        if let Some(value) = node.critic.font_size {
            push_sample(
                &mut samples.font_sizes,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.fontSize",
            );
        }
        if let Some(value) = node.critic.font_weight {
            push_sample(
                &mut samples.font_weights,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.fontWeight",
            );
        }
        if let Some(value) = node.critic.line_height {
            push_sample(
                &mut samples.line_heights,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.lineHeight",
            );
        }
        if node.critic.interactive {
            push_sample(
                &mut samples.control_heights,
                node.critic.rect.height,
                &node.critic.reference,
                &provenance,
                "geometry.control_height",
            );
        }
        for value in node
            .padding
            .iter()
            .copied()
            .take(MAX_STYLE_SAMPLES_PER_NODE)
        {
            push_sample(
                &mut samples.padding,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.padding",
            );
            push_sample(
                &mut samples.spacing,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.padding",
            );
        }
        for value in node.gaps.iter().copied().take(MAX_STYLE_SAMPLES_PER_NODE) {
            push_sample(
                &mut samples.gaps,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.gap",
            );
            push_sample(
                &mut samples.spacing,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.gap",
            );
        }
        for value in node.radius.iter().copied().take(MAX_STYLE_SAMPLES_PER_NODE) {
            push_sample(
                &mut samples.radius,
                value,
                &node.critic.reference,
                &provenance,
                "computed_style.borderRadius",
            );
        }
    }

    append_sibling_spacing(projected, &provenance, &mut samples.spacing);
    samples
}

fn append_sibling_spacing(
    projected: &[ProjectedNode],
    provenance: &EvidenceProvenance,
    spacing: &mut Vec<DesignMetricSample>,
) {
    let mut children: BTreeMap<&str, Vec<&ProjectedNode>> = BTreeMap::new();
    for node in projected {
        if let Some(parent) = node.parent.as_deref() {
            children.entry(parent).or_default().push(node);
        }
    }

    for siblings in children.values() {
        let mut horizontal = siblings.clone();
        horizontal.sort_by(|left, right| left.critic.rect.x.total_cmp(&right.critic.rect.x));
        for pair in horizontal.windows(2) {
            if vertical_overlap_ratio(&pair[0].critic.rect, &pair[1].critic.rect) >= 0.30 {
                let gap =
                    pair[1].critic.rect.x - (pair[0].critic.rect.x + pair[0].critic.rect.width);
                if gap.is_finite() && gap >= 0.0 {
                    push_sample(
                        spacing,
                        gap,
                        &pair[1].critic.reference,
                        provenance,
                        "geometry.sibling_gap_x",
                    );
                }
            }
        }

        let mut vertical = siblings.clone();
        vertical.sort_by(|left, right| left.critic.rect.y.total_cmp(&right.critic.rect.y));
        for pair in vertical.windows(2) {
            if horizontal_overlap_ratio(&pair[0].critic.rect, &pair[1].critic.rect) >= 0.30 {
                let gap =
                    pair[1].critic.rect.y - (pair[0].critic.rect.y + pair[0].critic.rect.height);
                if gap.is_finite() && gap >= 0.0 {
                    push_sample(
                        spacing,
                        gap,
                        &pair[1].critic.reference,
                        provenance,
                        "geometry.sibling_gap_y",
                    );
                }
            }
        }
    }
}

fn push_sample(
    target: &mut Vec<DesignMetricSample>,
    value: f64,
    reference: &str,
    provenance: &EvidenceProvenance,
    evidence_key: &str,
) {
    if !value.is_finite() || value < 0.0 {
        return;
    }
    let mut provenance = provenance.clone();
    provenance.evidence_keys = vec![evidence_key.into()];
    target.push(DesignMetricSample {
        value,
        reference: reference.to_owned(),
        provenance,
        status: EvidenceStatus::Observed,
    });
}

fn parse_viewport(value: &Value) -> Option<(f64, f64)> {
    let width = value.get("width")?.as_f64()?;
    let height = value.get("height")?.as_f64()?;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        .then_some((width, height))
}

fn parse_rect(value: &Value) -> Option<Rect> {
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    let width = value.get("width")?.as_f64()?;
    let height = value.get("height")?.as_f64()?;
    if [x, y, width, height]
        .iter()
        .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn parse_padding(style: &serde_json::Map<String, Value>) -> Vec<f64> {
    [
        "paddingTop",
        "paddingRight",
        "paddingBottom",
        "paddingLeft",
    ]
    .into_iter()
    .filter_map(|key| style_px(style, key))
    .filter(|value| *value >= 0.0)
    .collect()
}

fn parse_gaps(style: &serde_json::Map<String, Value>) -> Vec<f64> {
    let mut values = ["gap", "rowGap", "columnGap"]
        .into_iter()
        .filter_map(|key| style_px(style, key))
        .filter(|value| *value >= 0.0)
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= 0.01);
    values
}

fn parse_radius(style: &serde_json::Map<String, Value>) -> Vec<f64> {
    [
        "borderRadius",
        "borderTopLeftRadius",
        "borderTopRightRadius",
        "borderBottomRightRadius",
        "borderBottomLeftRadius",
    ]
    .into_iter()
    .filter_map(|key| style_px(style, key))
    .filter(|value| *value >= 0.0)
    .collect()
}

fn style_px(style: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    parse_css_px(style.get(key)?.as_str()?)
}

fn numeric_weight(style: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    let raw = style.get(key)?.as_str()?.trim();
    let value = raw.parse::<f64>().ok()?;
    (value.is_finite() && (1.0..=1_000.0).contains(&value)).then_some(value)
}

fn parse_css_px(value: &str) -> Option<f64> {
    let value = value
        .trim()
        .strip_suffix("px")?
        .trim()
        .parse::<f64>()
        .ok()?;
    value.is_finite().then_some(value)
}

fn parse_source_hint(value: Option<&Value>) -> Option<CriticSourceHint> {
    let object = value?.as_object()?;
    let file = object.get("file")?.as_str()?.trim();
    if file.is_empty()
        || file.len() > MAX_SOURCE_FILE_BYTES
        || file.starts_with('/')
        || file.contains('\\')
        || file
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || file.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }

    Some(CriticSourceHint {
        file: file.to_owned(),
        line: object
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        column: object
            .get("column")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        authority: SourceHintAuthority::ObservedRuntimeHint,
    })
}

fn vertical_overlap_ratio(left: &Rect, right: &Rect) -> f64 {
    let overlap =
        ((left.y + left.height).min(right.y + right.height) - left.y.max(right.y)).max(0.0);
    let smaller = left.height.min(right.height);
    if smaller <= 0.0 {
        0.0
    } else {
        overlap / smaller
    }
}

fn horizontal_overlap_ratio(left: &Rect, right: &Rect) -> f64 {
    let overlap =
        ((left.x + left.width).min(right.x + right.width) - left.x.max(right.x)).max(0.0);
    let smaller = left.width.min(right.width);
    if smaller <= 0.0 {
        0.0
    } else {
        overlap / smaller
    }
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

fn bounded_route(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_ROUTE_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return None;
    }
    Some(value)
}

fn f64_to_u32(value: f64) -> Option<u32> {
    if value.is_finite() && value > 0.0 && value <= u32::MAX as f64 {
        Some(value.round() as u32)
    } else {
        None
    }
}
