use std::collections::{BTreeMap, BTreeSet};

use localview_design_grammar::{MetricFamilies, ProjectDesignGrammar, ScaleFamily, nearest_family};
use localview_protocol::{ElementRef, Rect};
use serde::{Deserialize, Serialize};

pub const MAX_CRITIC_NODES: usize = 512;
pub const MAX_CRITIC_FINDINGS: usize = 128;
pub const MAX_FINDING_REFS: usize = 24;
pub const DESIGN_BASELINE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CriticEvidenceClass {
    Deterministic,
    Heuristic,
    Subjective,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CriticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriticMeasurement {
    pub name: String,
    pub measured: f64,
    pub unit: String,
    pub expected: Option<f64>,
    pub deviation: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceHintAuthority {
    ObservedRuntimeHint,
    ExactDeclarationPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CriticSourceHint {
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub authority: SourceHintAuthority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriticFinding {
    pub id: String,
    pub code: String,
    pub class: CriticEvidenceClass,
    pub severity: CriticSeverity,
    pub confidence: f64,
    pub affected_refs: Vec<ElementRef>,
    pub evidence_summary: String,
    pub measured_values: Vec<CriticMeasurement>,
    pub related_grammar_family: Option<String>,
    pub source_hints: Vec<CriticSourceHint>,
    pub classification_reason: String,
}

impl CriticFinding {
    pub fn can_fail_ci(&self) -> bool {
        self.class == CriticEvidenceClass::Deterministic && self.severity == CriticSeverity::Error
    }

    pub const fn can_trigger_automatic_fix(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriticNode {
    pub reference: ElementRef,
    pub rect: Rect,
    pub interactive: bool,
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub line_height: Option<f64>,
    pub contrast: Option<f64>,
    pub source_hint: Option<CriticSourceHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum FeatureState<T> {
    Available(T),
    Unavailable { reason: String },
}

impl<T> Default for FeatureState<T> {
    fn default() -> Self {
        Self::Unavailable {
            reason: "no_evidence".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DensityFeatures {
    pub occupied_area_ratio: f64,
    pub whitespace_ratio: f64,
    pub control_count: usize,
    pub node_count: usize,
    pub typography_node_count: usize,
    pub controls_per_100k_px: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BalanceFeatures {
    pub left_visual_mass: f64,
    pub right_visual_mass: f64,
    pub top_visual_mass: f64,
    pub bottom_visual_mass: f64,
    pub horizontal_imbalance: f64,
    pub vertical_imbalance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HierarchyEntry {
    pub reference: ElementRef,
    pub salience: f64,
    pub font_size: Option<f64>,
    pub font_weight: Option<f64>,
    pub area_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HierarchyFeatures {
    pub entries: Vec<HierarchyEntry>,
    pub salience_spread: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VisualCriticReport {
    pub density: FeatureState<DensityFeatures>,
    pub balance: FeatureState<BalanceFeatures>,
    pub hierarchy: FeatureState<HierarchyFeatures>,
    pub findings: Vec<CriticFinding>,
    pub analyzed_nodes: usize,
    pub input_truncated: bool,
}

pub fn analyze_visual_critic(
    nodes: &[CriticNode],
    viewport: (f64, f64),
    grammar: &ProjectDesignGrammar,
) -> VisualCriticReport {
    let input_truncated = nodes.len() > MAX_CRITIC_NODES;
    let nodes = &nodes[..nodes.len().min(MAX_CRITIC_NODES)];
    if !valid_viewport(viewport) {
        return VisualCriticReport {
            density: unavailable("invalid_viewport"),
            balance: unavailable("invalid_viewport"),
            hierarchy: unavailable("invalid_viewport"),
            analyzed_nodes: nodes.len(),
            input_truncated,
            ..Default::default()
        };
    }

    let density = density_features(nodes, viewport);
    let balance = balance_features(nodes, viewport);
    let hierarchy = hierarchy_features(nodes, viewport);
    let mut findings = Vec::new();

    if let FeatureState::Available(value) = &density {
        if value.occupied_area_ratio >= 0.82
            && (value.node_count >= 20 || value.controls_per_100k_px >= 12.0)
        {
            push_finding(
                &mut findings,
                CriticFinding {
                    id: "visual.density.high".into(),
                    code: "excessive_density_candidate".into(),
                    class: CriticEvidenceClass::Heuristic,
                    severity: CriticSeverity::Warning,
                    confidence: 0.72,
                    affected_refs: nodes
                        .iter()
                        .take(MAX_FINDING_REFS)
                        .map(|node| node.reference.clone())
                        .collect(),
                    evidence_summary:
                        "Measured occupied-area proxy and control concentration exceed bounded thresholds"
                            .into(),
                    measured_values: vec![
                        measurement(
                            "occupied_area_ratio",
                            value.occupied_area_ratio,
                            "ratio",
                            Some(0.82),
                        ),
                        measurement(
                            "controls_per_100k_px",
                            value.controls_per_100k_px,
                            "count_per_100k_px",
                            Some(12.0),
                        ),
                    ],
                    related_grammar_family: None,
                    source_hints: Vec::new(),
                    classification_reason:
                        "Density features are measured, but whether the composition is too dense depends on design intent"
                            .into(),
                },
            );
        }
    }

    if let FeatureState::Available(value) = &balance {
        let imbalance = value.horizontal_imbalance.max(value.vertical_imbalance);
        if imbalance >= 0.40 {
            push_finding(
                &mut findings,
                CriticFinding {
                    id: "visual.balance.mass".into(),
                    code: "visual_mass_imbalance_candidate".into(),
                    class: CriticEvidenceClass::Heuristic,
                    severity: CriticSeverity::Info,
                    confidence: 0.62,
                    affected_refs: nodes
                        .iter()
                        .take(MAX_FINDING_REFS)
                        .map(|node| node.reference.clone())
                        .collect(),
                    evidence_summary:
                        "Approximate visual mass is uneven across the observed viewport halves"
                            .into(),
                    measured_values: vec![measurement(
                        "max_axis_imbalance",
                        imbalance,
                        "ratio",
                        Some(0.40),
                    )],
                    related_grammar_family: None,
                    source_hints: Vec::new(),
                    classification_reason:
                        "Visual mass is a geometry approximation; imbalance is not a deterministic defect"
                            .into(),
                },
            );
        }
    }

    if let FeatureState::Available(value) = &hierarchy {
        if value.entries.len() >= 4 && value.salience_spread <= 0.10 {
            push_finding(
                &mut findings,
                CriticFinding {
                    id: "visual.hierarchy.weak".into(),
                    code: "weak_hierarchy_candidate".into(),
                    class: CriticEvidenceClass::Heuristic,
                    severity: CriticSeverity::Info,
                    confidence: 0.66,
                    affected_refs: value
                        .entries
                        .iter()
                        .take(MAX_FINDING_REFS)
                        .map(|entry| entry.reference.clone())
                        .collect(),
                    evidence_summary:
                        "Observed geometry and typography produce a narrow salience spread".into(),
                    measured_values: vec![measurement(
                        "salience_spread",
                        value.salience_spread,
                        "ratio",
                        Some(0.10),
                    )],
                    related_grammar_family: None,
                    source_hints: Vec::new(),
                    classification_reason:
                        "Hierarchy quality depends on intent; salience spread is a bounded heuristic"
                            .into(),
                },
            );
        }
    }

    append_metric_drift(
        nodes,
        &grammar.type_size_families,
        "type_size",
        |node| node.font_size,
        &mut findings,
    );
    append_metric_drift(
        nodes,
        &grammar.font_weight_families,
        "font_weight",
        |node| node.font_weight,
        &mut findings,
    );
    append_metric_drift(
        nodes,
        &grammar.line_height_families,
        "line_height",
        |node| node.line_height,
        &mut findings,
    );
    append_control_height_drift(nodes, &grammar.control_height_families, &mut findings);

    VisualCriticReport {
        density,
        balance,
        hierarchy,
        findings,
        analyzed_nodes: nodes.len(),
        input_truncated,
    }
}

fn unavailable<T>(reason: &str) -> FeatureState<T> {
    FeatureState::Unavailable {
        reason: reason.into(),
    }
}

fn valid_viewport(viewport: (f64, f64)) -> bool {
    viewport.0.is_finite() && viewport.1.is_finite() && viewport.0 > 0.0 && viewport.1 > 0.0
}

fn valid_rect(rect: &Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
}

fn clipped_area(rect: &Rect, viewport: (f64, f64)) -> f64 {
    if !valid_rect(rect) {
        return 0.0;
    }
    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let right = (rect.x + rect.width).min(viewport.0);
    let bottom = (rect.y + rect.height).min(viewport.1);
    (right - left).max(0.0) * (bottom - top).max(0.0)
}

fn density_features(nodes: &[CriticNode], viewport: (f64, f64)) -> FeatureState<DensityFeatures> {
    if nodes.is_empty() {
        return unavailable("no_nodes");
    }
    let viewport_area = viewport.0 * viewport.1;
    let occupied_proxy = nodes
        .iter()
        .map(|node| clipped_area(&node.rect, viewport))
        .sum::<f64>();
    let occupied_area_ratio = (occupied_proxy / viewport_area).clamp(0.0, 1.0);
    let control_count = nodes.iter().filter(|node| node.interactive).count();
    FeatureState::Available(DensityFeatures {
        occupied_area_ratio,
        whitespace_ratio: (1.0 - occupied_area_ratio).clamp(0.0, 1.0),
        control_count,
        node_count: nodes.len(),
        typography_node_count: nodes.iter().filter(|node| node.font_size.is_some()).count(),
        controls_per_100k_px: control_count as f64 * 100_000.0 / viewport_area,
    })
}

fn balance_features(nodes: &[CriticNode], viewport: (f64, f64)) -> FeatureState<BalanceFeatures> {
    let usable = nodes
        .iter()
        .filter(|node| valid_rect(&node.rect))
        .collect::<Vec<_>>();
    if usable.is_empty() {
        return unavailable("no_geometry");
    }

    let (mut left, mut right, mut top, mut bottom) = (0.0, 0.0, 0.0, 0.0);
    for node in usable {
        let mass = clipped_area(&node.rect, viewport);
        let center_x = node.rect.x + node.rect.width / 2.0;
        let center_y = node.rect.y + node.rect.height / 2.0;
        if center_x <= viewport.0 / 2.0 {
            left += mass;
        } else {
            right += mass;
        }
        if center_y <= viewport.1 / 2.0 {
            top += mass;
        } else {
            bottom += mass;
        }
    }

    FeatureState::Available(BalanceFeatures {
        left_visual_mass: left,
        right_visual_mass: right,
        top_visual_mass: top,
        bottom_visual_mass: bottom,
        horizontal_imbalance: normalized_difference(left, right),
        vertical_imbalance: normalized_difference(top, bottom),
    })
}

fn normalized_difference(left: f64, right: f64) -> f64 {
    let total = left + right;
    if total <= f64::EPSILON {
        0.0
    } else {
        (left - right).abs() / total
    }
}

fn hierarchy_features(nodes: &[CriticNode], viewport: (f64, f64)) -> FeatureState<HierarchyFeatures> {
    let max_font = nodes
        .iter()
        .filter_map(|node| node.font_size)
        .filter(|value| value.is_finite() && *value > 0.0)
        .max_by(f64::total_cmp);
    let max_weight = nodes
        .iter()
        .filter_map(|node| node.font_weight)
        .filter(|value| value.is_finite() && *value > 0.0)
        .max_by(f64::total_cmp);
    if max_font.is_none() && max_weight.is_none() {
        return unavailable("typography_evidence_unavailable");
    }

    let viewport_area = viewport.0 * viewport.1;
    let mut entries = nodes
        .iter()
        .filter(|node| valid_rect(&node.rect))
        .map(|node| {
            let font = node
                .font_size
                .zip(max_font)
                .map_or(0.0, |(value, max)| (value / max).clamp(0.0, 1.0));
            let weight = node
                .font_weight
                .zip(max_weight)
                .map_or(0.0, |(value, max)| (value / max).clamp(0.0, 1.0));
            let area_ratio =
                (clipped_area(&node.rect, viewport) / viewport_area).clamp(0.0, 1.0);
            let contrast = node
                .contrast
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map_or(0.0, |value| (value / 7.0).clamp(0.0, 1.0));
            let top_bias =
                (1.0 - ((node.rect.y + node.rect.height / 2.0) / viewport.1)).clamp(0.0, 1.0);
            let interaction = if node.interactive { 1.0 } else { 0.0 };
            let salience = (font * 0.40
                + weight * 0.18
                + area_ratio.sqrt() * 0.20
                + contrast * 0.10
                + top_bias * 0.07
                + interaction * 0.05)
                .clamp(0.0, 1.0);
            HierarchyEntry {
                reference: node.reference.clone(),
                salience,
                font_size: node.font_size,
                font_weight: node.font_weight,
                area_ratio,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .salience
            .total_cmp(&left.salience)
            .then_with(|| left.reference.cmp(&right.reference))
    });
    entries.truncate(64);
    let salience_spread = match (entries.first(), entries.last()) {
        (Some(first), Some(last)) => (first.salience - last.salience).max(0.0),
        _ => 0.0,
    };
    FeatureState::Available(HierarchyFeatures {
        entries,
        salience_spread,
    })
}

fn append_metric_drift(
    nodes: &[CriticNode],
    metric: &MetricFamilies,
    metric_name: &str,
    value: fn(&CriticNode) -> Option<f64>,
    findings: &mut Vec<CriticFinding>,
) {
    for node in nodes {
        let Some(measured) = value(node).filter(|value| value.is_finite() && *value >= 0.0) else {
            continue;
        };
        let Some((family, deviation)) = nearest_family(measured, metric) else {
            continue;
        };
        if !established_family(family) || deviation <= family_tolerance(family, metric_name) {
            continue;
        }
        push_finding(
            findings,
            scale_drift_finding(node, metric_name, measured, family, deviation),
        );
    }
}

fn append_control_height_drift(
    nodes: &[CriticNode],
    metric: &MetricFamilies,
    findings: &mut Vec<CriticFinding>,
) {
    for node in nodes
        .iter()
        .filter(|node| node.interactive && valid_rect(&node.rect))
    {
        let measured = node.rect.height;
        let Some((family, deviation)) = nearest_family(measured, metric) else {
            continue;
        };
        if established_family(family) && deviation > family_tolerance(family, "control_height") {
            push_finding(
                findings,
                scale_drift_finding(node, "control_height", measured, family, deviation),
            );
        }
    }
}

fn scale_drift_finding(
    node: &CriticNode,
    metric_name: &str,
    measured: f64,
    family: &ScaleFamily,
    deviation: f64,
) -> CriticFinding {
    CriticFinding {
        id: format!("visual.scale.{metric_name}.{}", node.reference),
        code: "observed_scale_deviation".into(),
        class: CriticEvidenceClass::Deterministic,
        severity: CriticSeverity::Info,
        confidence: family.confidence,
        affected_refs: vec![node.reference.clone()],
        evidence_summary: format!(
            "Observed {metric_name} value differs from the nearest repeated project family"
        ),
        measured_values: vec![CriticMeasurement {
            name: metric_name.into(),
            measured,
            unit: if metric_name == "font_weight" {
                "weight".into()
            } else {
                "px".into()
            },
            expected: Some(family.center),
            deviation: Some(deviation),
        }],
        related_grammar_family: Some(format!("{metric_name}:{:.3}", family.center)),
        source_hints: node.source_hint.clone().into_iter().collect(),
        classification_reason:
            "The measured value and repeated family are bounded observed evidence; this states deviation only, not aesthetic wrongness"
                .into(),
    }
}

fn established_family(family: &ScaleFamily) -> bool {
    family.sample_count >= 3 && family.support_ratio >= 0.20 && family.confidence >= 0.45
}

fn family_tolerance(family: &ScaleFamily, metric_name: &str) -> f64 {
    let spread = (family.maximum - family.minimum).abs();
    match metric_name {
        "font_weight" => spread.max(25.0),
        "control_height" => spread.max(2.0),
        _ => spread.max(1.0),
    }
}

fn measurement(
    name: &str,
    measured: f64,
    unit: &str,
    expected: Option<f64>,
) -> CriticMeasurement {
    CriticMeasurement {
        name: name.into(),
        measured,
        unit: unit.into(),
        expected,
        deviation: expected.map(|expected| (measured - expected).abs()),
    }
}

fn push_finding(findings: &mut Vec<CriticFinding>, mut finding: CriticFinding) {
    if findings.len() >= MAX_CRITIC_FINDINGS {
        return;
    }
    finding.confidence = finding.confidence.clamp(0.0, 1.0);
    finding.affected_refs.sort();
    finding.affected_refs.dedup();
    finding.affected_refs.truncate(MAX_FINDING_REFS);
    findings.push(finding);
}

pub fn subjective_finding(
    id: impl Into<String>,
    code: impl Into<String>,
    affected_refs: Vec<ElementRef>,
    summary: impl Into<String>,
    confidence: f64,
) -> CriticFinding {
    let mut finding = CriticFinding {
        id: id.into(),
        code: code.into(),
        class: CriticEvidenceClass::Subjective,
        severity: CriticSeverity::Info,
        confidence: confidence.clamp(0.0, 1.0),
        affected_refs,
        evidence_summary: summary.into(),
        measured_values: Vec::new(),
        related_grammar_family: None,
        source_hints: Vec::new(),
        classification_reason:
            "Aesthetic preference is explicitly subjective and is never promoted by confidence"
                .into(),
    };
    finding.affected_refs.sort();
    finding.affected_refs.dedup();
    finding.affected_refs.truncate(MAX_FINDING_REFS);
    finding
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriticOverlayItem {
    pub finding_id: String,
    pub class: CriticEvidenceClass,
    pub confidence: f64,
    pub affected_refs: Vec<ElementRef>,
    pub measured_deviation: Option<f64>,
    pub source_hint: Option<CriticSourceHint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CriticOverlayModel {
    pub items: Vec<CriticOverlayItem>,
    pub selected_finding_id: Option<String>,
    pub suppress_during_evidence_capture: bool,
}

pub fn overlay_model(report: &VisualCriticReport, selected: Option<&str>) -> CriticOverlayModel {
    CriticOverlayModel {
        items: report
            .findings
            .iter()
            .map(|finding| CriticOverlayItem {
                finding_id: finding.id.clone(),
                class: finding.class,
                confidence: finding.confidence,
                affected_refs: finding.affected_refs.clone(),
                measured_deviation: finding
                    .measured_values
                    .iter()
                    .find_map(|value| value.deviation),
                source_hint: finding.source_hints.first().cloned(),
            })
            .collect(),
        selected_finding_id: selected.map(str::to_owned),
        suppress_during_evidence_capture: true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DesignDistributionSummary {
    pub occupied_area_ratio: Option<f64>,
    pub horizontal_imbalance: Option<f64>,
    pub vertical_imbalance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HierarchyBaselineSummary {
    pub salience_spread: Option<f64>,
    pub leading_refs: Vec<ElementRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignGrammarBaseline {
    pub schema_version: u16,
    pub grammar: ProjectDesignGrammar,
    pub distribution: DesignDistributionSummary,
    pub hierarchy: HierarchyBaselineSummary,
    pub deterministic_facts: Vec<String>,
    pub heuristic_facts: Vec<String>,
}

pub fn build_design_baseline(
    grammar: ProjectDesignGrammar,
    report: &VisualCriticReport,
) -> DesignGrammarBaseline {
    let distribution = DesignDistributionSummary {
        occupied_area_ratio: available_density(report).map(|value| value.occupied_area_ratio),
        horizontal_imbalance: available_balance(report).map(|value| value.horizontal_imbalance),
        vertical_imbalance: available_balance(report).map(|value| value.vertical_imbalance),
    };
    let hierarchy = available_hierarchy(report).map_or_else(
        HierarchyBaselineSummary::default,
        |value| HierarchyBaselineSummary {
            salience_spread: Some(value.salience_spread),
            leading_refs: value
                .entries
                .iter()
                .take(8)
                .map(|entry| entry.reference.clone())
                .collect(),
        },
    );
    DesignGrammarBaseline {
        schema_version: DESIGN_BASELINE_SCHEMA_VERSION,
        grammar,
        distribution,
        hierarchy,
        deterministic_facts: fact_codes(report, CriticEvidenceClass::Deterministic),
        heuristic_facts: fact_codes(report, CriticEvidenceClass::Heuristic),
    }
}

fn available_density(report: &VisualCriticReport) -> Option<&DensityFeatures> {
    match &report.density {
        FeatureState::Available(value) => Some(value),
        FeatureState::Unavailable { .. } => None,
    }
}

fn available_balance(report: &VisualCriticReport) -> Option<&BalanceFeatures> {
    match &report.balance {
        FeatureState::Available(value) => Some(value),
        FeatureState::Unavailable { .. } => None,
    }
}

fn available_hierarchy(report: &VisualCriticReport) -> Option<&HierarchyFeatures> {
    match &report.hierarchy {
        FeatureState::Available(value) => Some(value),
        FeatureState::Unavailable { .. } => None,
    }
}

fn fact_codes(report: &VisualCriticReport, class: CriticEvidenceClass) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|finding| finding.class == class)
        .map(|finding| finding.code.clone())
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FamilyChange {
    pub metric: String,
    pub before: Option<f64>,
    pub after: Option<f64>,
    pub delta: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DesignBaselineDiff {
    pub added_families: Vec<FamilyChange>,
    pub removed_families: Vec<FamilyChange>,
    pub scale_drift: Vec<FamilyChange>,
    pub distribution_changes: BTreeMap<String, f64>,
    pub hierarchy_regressions: Vec<String>,
    pub confidence_changes: BTreeMap<String, f64>,
    pub inconclusive: Vec<String>,
}

pub fn diff_design_baselines(
    before: &DesignGrammarBaseline,
    after: &DesignGrammarBaseline,
) -> DesignBaselineDiff {
    let mut diff = DesignBaselineDiff::default();
    if before.schema_version != after.schema_version {
        diff.inconclusive.push("schema_version_changed".into());
        return diff;
    }

    for (metric, old, new) in grammar_metrics(&before.grammar, &after.grammar) {
        diff_metric(metric, old, new, &mut diff);
    }
    compare_distribution(
        "occupied_area_ratio",
        before.distribution.occupied_area_ratio,
        after.distribution.occupied_area_ratio,
        0.08,
        &mut diff,
    );
    compare_distribution(
        "horizontal_imbalance",
        before.distribution.horizontal_imbalance,
        after.distribution.horizontal_imbalance,
        0.12,
        &mut diff,
    );
    compare_distribution(
        "vertical_imbalance",
        before.distribution.vertical_imbalance,
        after.distribution.vertical_imbalance,
        0.12,
        &mut diff,
    );
    match (
        before.hierarchy.salience_spread,
        after.hierarchy.salience_spread,
    ) {
        (Some(old), Some(new)) if old - new >= 0.12 => {
            diff.hierarchy_regressions
                .push("salience_spread_reduced".into());
        }
        (None, _) | (_, None) => diff.inconclusive.push("hierarchy_evidence_missing".into()),
        _ => {}
    }

    diff.added_families.sort_by(change_order);
    diff.removed_families.sort_by(change_order);
    diff.scale_drift.sort_by(change_order);
    diff.inconclusive.sort();
    diff.inconclusive.dedup();
    diff
}

type MetricPair<'a> = (&'static str, &'a MetricFamilies, &'a MetricFamilies);

fn grammar_metrics<'a>(
    before: &'a ProjectDesignGrammar,
    after: &'a ProjectDesignGrammar,
) -> [MetricPair<'a>; 8] {
    [
        ("spacing", &before.spacing_families, &after.spacing_families),
        ("type_size", &before.type_size_families, &after.type_size_families),
        (
            "font_weight",
            &before.font_weight_families,
            &after.font_weight_families,
        ),
        (
            "line_height",
            &before.line_height_families,
            &after.line_height_families,
        ),
        (
            "control_height",
            &before.control_height_families,
            &after.control_height_families,
        ),
        ("radius", &before.radius_families, &after.radius_families),
        ("gap", &before.gap_families, &after.gap_families),
        ("padding", &before.padding_families, &after.padding_families),
    ]
}

fn diff_metric(
    metric: &str,
    before: &MetricFamilies,
    after: &MetricFamilies,
    diff: &mut DesignBaselineDiff,
) {
    if before.families().is_empty() || after.families().is_empty() {
        if before.families().is_empty() && after.families().is_empty() {
            diff.inconclusive
                .push(format!("{metric}_evidence_unavailable"));
        } else if before.families().is_empty() {
            for family in after.families() {
                diff.added_families.push(FamilyChange {
                    metric: metric.into(),
                    before: None,
                    after: Some(family.center),
                    delta: None,
                });
            }
        } else {
            for family in before.families() {
                diff.removed_families.push(FamilyChange {
                    metric: metric.into(),
                    before: Some(family.center),
                    after: None,
                    delta: None,
                });
            }
        }
        return;
    }

    let mut matched_after = BTreeSet::new();
    for old in before.families() {
        let nearest = after.families().iter().enumerate().min_by(|(_, left), (_, right)| {
            (old.center - left.center)
                .abs()
                .total_cmp(&(old.center - right.center).abs())
        });
        let Some((index, new)) = nearest else {
            continue;
        };
        let delta = new.center - old.center;
        if delta.abs() <= 0.75 {
            matched_after.insert(index);
            let confidence_delta = new.confidence - old.confidence;
            if confidence_delta.abs() >= 0.15 {
                diff.confidence_changes
                    .insert(format!("{metric}:{:.3}", old.center), confidence_delta);
            }
        } else if delta.abs() <= 4.0 {
            matched_after.insert(index);
            diff.scale_drift.push(FamilyChange {
                metric: metric.into(),
                before: Some(old.center),
                after: Some(new.center),
                delta: Some(delta),
            });
        } else {
            diff.removed_families.push(FamilyChange {
                metric: metric.into(),
                before: Some(old.center),
                after: None,
                delta: None,
            });
        }
    }
    for (index, family) in after.families().iter().enumerate() {
        if !matched_after.contains(&index) {
            diff.added_families.push(FamilyChange {
                metric: metric.into(),
                before: None,
                after: Some(family.center),
                delta: None,
            });
        }
    }
}

fn compare_distribution(
    key: &str,
    before: Option<f64>,
    after: Option<f64>,
    threshold: f64,
    diff: &mut DesignBaselineDiff,
) {
    match (before, after) {
        (Some(old), Some(new)) if (new - old).abs() >= threshold => {
            diff.distribution_changes.insert(key.into(), new - old);
        }
        (None, _) | (_, None) => diff.inconclusive.push(format!("{key}_evidence_missing")),
        _ => {}
    }
}

fn change_order(left: &FamilyChange, right: &FamilyChange) -> std::cmp::Ordering {
    left.metric
        .cmp(&right.metric)
        .then_with(|| option_total_cmp(left.before.or(left.after), right.before.or(right.after)))
}

fn option_total_cmp(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}
