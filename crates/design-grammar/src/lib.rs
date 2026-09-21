#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::ElementRef;
use serde::{Deserialize, Serialize};

pub const MAX_GRAMMAR_SAMPLES_PER_METRIC: usize = 2_048;
pub const MAX_REFS_PER_FAMILY: usize = 24;
pub const MIN_FAMILY_SAMPLES: usize = 2;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Observed,
    Inferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EvidenceProvenance {
    pub snapshot_seq: Option<u64>,
    pub snapshot_version: Option<u64>,
    pub route: Option<String>,
    pub viewport_width: Option<u32>,
    pub viewport_height: Option<u32>,
    pub evidence_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignMetricSample {
    pub value: f64,
    pub reference: ElementRef,
    pub provenance: EvidenceProvenance,
    pub status: EvidenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DesignEvidenceSamples {
    pub spacing: Vec<DesignMetricSample>,
    pub font_sizes: Vec<DesignMetricSample>,
    pub font_weights: Vec<DesignMetricSample>,
    pub line_heights: Vec<DesignMetricSample>,
    pub control_heights: Vec<DesignMetricSample>,
    pub radius: Vec<DesignMetricSample>,
    pub gaps: Vec<DesignMetricSample>,
    pub padding: Vec<DesignMetricSample>,
    pub observed_nodes: usize,
    pub input_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScaleFamily {
    pub center: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub sample_count: usize,
    pub support_ratio: f64,
    pub refs: Vec<ElementRef>,
    pub provenance: Vec<EvidenceProvenance>,
    pub confidence: f64,
    pub status: EvidenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum MetricFamilies {
    Available {
        sample_count: usize,
        families: Vec<ScaleFamily>,
    },
    Unavailable {
        reason: String,
    },
}

impl Default for MetricFamilies {
    fn default() -> Self {
        Self::Unavailable {
            reason: "no_evidence".into(),
        }
    }
}

impl MetricFamilies {
    pub fn families(&self) -> &[ScaleFamily] {
        match self {
            Self::Available { families, .. } => families,
            Self::Unavailable { .. } => &[],
        }
    }

    pub fn sample_count(&self) -> usize {
        match self {
            Self::Available { sample_count, .. } => *sample_count,
            Self::Unavailable { .. } => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProjectDesignGrammar {
    pub spacing_families: MetricFamilies,
    pub type_size_families: MetricFamilies,
    pub font_weight_families: MetricFamilies,
    pub line_height_families: MetricFamilies,
    pub control_height_families: MetricFamilies,
    pub radius_families: MetricFamilies,
    pub gap_families: MetricFamilies,
    pub padding_families: MetricFamilies,
    pub observed_nodes: usize,
    pub input_truncated: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ExtractionPolicy {
    pub max_samples_per_metric: usize,
    pub max_refs_per_family: usize,
    pub minimum_family_samples: usize,
    pub spacing_epsilon: f64,
    pub type_size_epsilon: f64,
    pub font_weight_epsilon: f64,
    pub line_height_epsilon: f64,
    pub control_height_epsilon: f64,
    pub radius_epsilon: f64,
}

impl Default for ExtractionPolicy {
    fn default() -> Self {
        Self {
            max_samples_per_metric: MAX_GRAMMAR_SAMPLES_PER_METRIC,
            max_refs_per_family: MAX_REFS_PER_FAMILY,
            minimum_family_samples: MIN_FAMILY_SAMPLES,
            spacing_epsilon: 0.75,
            type_size_epsilon: 0.75,
            font_weight_epsilon: 1.0,
            line_height_epsilon: 0.75,
            control_height_epsilon: 1.0,
            radius_epsilon: 0.75,
        }
    }
}

pub fn extract_project_grammar(
    samples: &DesignEvidenceSamples,
    policy: ExtractionPolicy,
) -> ProjectDesignGrammar {
    ProjectDesignGrammar {
        spacing_families: extract(&samples.spacing, policy.spacing_epsilon, policy),
        type_size_families: extract(&samples.font_sizes, policy.type_size_epsilon, policy),
        font_weight_families: extract(
            &samples.font_weights,
            policy.font_weight_epsilon,
            policy,
        ),
        line_height_families: extract(
            &samples.line_heights,
            policy.line_height_epsilon,
            policy,
        ),
        control_height_families: extract(
            &samples.control_heights,
            policy.control_height_epsilon,
            policy,
        ),
        radius_families: extract(&samples.radius, policy.radius_epsilon, policy),
        gap_families: extract(&samples.gaps, policy.spacing_epsilon, policy),
        padding_families: extract(&samples.padding, policy.spacing_epsilon, policy),
        observed_nodes: samples.observed_nodes,
        input_truncated: samples.input_truncated,
    }
}

fn extract(
    samples: &[DesignMetricSample],
    epsilon: f64,
    policy: ExtractionPolicy,
) -> MetricFamilies {
    let mut bounded = samples
        .iter()
        .filter(|sample| sample.value.is_finite() && sample.value >= 0.0)
        .take(policy.max_samples_per_metric)
        .cloned()
        .collect::<Vec<_>>();
    if bounded.is_empty() {
        return MetricFamilies::Unavailable {
            reason: "no_live_evidence".into(),
        };
    }
    if bounded.len() < policy.minimum_family_samples {
        return MetricFamilies::Unavailable {
            reason: "insufficient_samples".into(),
        };
    }

    let sample_count = bounded.len();
    bounded.sort_by(|left, right| left.value.total_cmp(&right.value));
    let mut clusters: Vec<Vec<DesignMetricSample>> = Vec::new();
    for sample in bounded {
        if let Some(last) = clusters.last_mut() {
            let center = last.iter().map(|entry| entry.value).sum::<f64>() / last.len() as f64;
            if (sample.value - center).abs() <= epsilon {
                last.push(sample);
                continue;
            }
        }
        clusters.push(vec![sample]);
    }

    let mut families = clusters
        .into_iter()
        .filter(|cluster| cluster.len() >= policy.minimum_family_samples)
        .map(|cluster| family(cluster, sample_count, policy.max_refs_per_family))
        .collect::<Vec<_>>();
    families.sort_by(|left, right| left.center.total_cmp(&right.center));
    if families.is_empty() {
        MetricFamilies::Unavailable {
            reason: "no_repeated_family".into(),
        }
    } else {
        MetricFamilies::Available {
            sample_count,
            families,
        }
    }
}

fn family(cluster: Vec<DesignMetricSample>, total: usize, max_refs: usize) -> ScaleFamily {
    let center =
        cluster.iter().map(|sample| sample.value).sum::<f64>() / cluster.len() as f64;
    let minimum = cluster
        .iter()
        .map(|sample| sample.value)
        .min_by(f64::total_cmp)
        .unwrap_or(center);
    let maximum = cluster
        .iter()
        .map(|sample| sample.value)
        .max_by(f64::total_cmp)
        .unwrap_or(center);
    let sample_count = cluster.len();
    let support_ratio = sample_count as f64 / total as f64;
    let confidence =
        (support_ratio * 0.65 + (sample_count as f64 / 8.0).min(1.0) * 0.35).clamp(0.0, 0.99);

    let refs = cluster
        .iter()
        .map(|sample| sample.reference.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(max_refs)
        .collect();
    let mut seen = BTreeSet::new();
    let provenance = cluster
        .iter()
        .filter(|sample| seen.insert(provenance_key(&sample.provenance)))
        .take(8)
        .map(|sample| sample.provenance.clone())
        .collect();

    ScaleFamily {
        center,
        minimum,
        maximum,
        sample_count,
        support_ratio,
        refs,
        provenance,
        confidence,
        status: EvidenceStatus::Inferred,
    }
}

fn provenance_key(value: &EvidenceProvenance) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{}",
        value.snapshot_seq,
        value.snapshot_version,
        value.route,
        value.viewport_width,
        value.viewport_height,
        value.evidence_keys.join(",")
    )
}

pub fn nearest_family(value: f64, metric: &MetricFamilies) -> Option<(&ScaleFamily, f64)> {
    value.is_finite().then_some(())?;
    metric
        .families()
        .iter()
        .map(|family| (family, (value - family.center).abs()))
        .min_by(|left, right| left.1.total_cmp(&right.1))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsiveVariationState {
    Stable,
    Varies,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsiveGrammarSnapshot {
    pub viewport_width: u32,
    pub grammar: ProjectDesignGrammar,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsiveMetricVariation {
    pub metric: String,
    pub state: ResponsiveVariationState,
    pub observed_widths: Vec<u32>,
    pub family_centers_by_width: BTreeMap<u32, Vec<f64>>,
}

type GrammarMetricAccessor = fn(&ProjectDesignGrammar) -> &MetricFamilies;
type GrammarMetric = (&'static str, GrammarMetricAccessor);

pub fn responsive_variation(
    snapshots: &[ResponsiveGrammarSnapshot],
) -> Vec<ResponsiveMetricVariation> {
    let mut snapshots = snapshots.to_vec();
    snapshots.sort_by_key(|snapshot| snapshot.viewport_width);
    let metrics: [GrammarMetric; 5] = [
        ("spacing", |value| &value.spacing_families),
        ("type_size", |value| &value.type_size_families),
        ("font_weight", |value| &value.font_weight_families),
        ("line_height", |value| &value.line_height_families),
        ("control_height", |value| &value.control_height_families),
    ];

    metrics
        .into_iter()
        .map(|(metric, get)| {
            let family_centers_by_width = snapshots
                .iter()
                .filter_map(|snapshot| {
                    let centers = get(&snapshot.grammar)
                        .families()
                        .iter()
                        .map(|family| family.center)
                        .collect::<Vec<_>>();
                    (!centers.is_empty()).then_some((snapshot.viewport_width, centers))
                })
                .collect::<BTreeMap<_, _>>();
            let observed_widths = family_centers_by_width.keys().copied().collect();
            let state = if family_centers_by_width.len() < 2 {
                ResponsiveVariationState::Unavailable
            } else {
                let mut values = family_centers_by_width.values();
                let first = values.next().cloned().unwrap_or_default();
                if values.all(|current| same_centers(&first, current)) {
                    ResponsiveVariationState::Stable
                } else {
                    ResponsiveVariationState::Varies
                }
            };
            ResponsiveMetricVariation {
                metric: metric.into(),
                state,
                observed_widths,
                family_centers_by_width,
            }
        })
        .collect()
}

fn same_centers(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| (left - right).abs() <= 0.75)
}
