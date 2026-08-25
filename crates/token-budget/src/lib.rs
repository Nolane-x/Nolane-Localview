#![forbid(unsafe_code)]

use std::{cmp::Ordering, error::Error, fmt};

use localview_protocol::{DetailLevel, Rect, TokenBudget};
use serde::{Deserialize, Serialize};

pub fn approximate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

pub fn serialize_with_budget<T: Serialize>(
    value: &T,
    budget: &TokenBudget,
) -> serde_json::Value {
    let full = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    let serialized = serde_json::to_string(&full).unwrap_or_default();
    if approximate_tokens(&serialized) <= budget.max_tokens {
        return full;
    }
    match budget.detail {
        DetailLevel::Deep => full,
        DetailLevel::Normal => trim_json(full, budget.max_tokens * 4),
        DetailLevel::Minimal => summary_json(full),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VisualPacketBudget {
    pub text: TokenBudget,
    pub image_regions: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VisualPacketSource {
    ChangedRegion,
    ProgressiveElement,
    ProgressiveComponent,
    ProgressiveSection,
    ViewportFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualPacketCandidate {
    pub source: VisualPacketSource,
    pub rect: Rect,
    pub information_gain_milli: u16,
    pub confidence_milli: u16,
    pub relevance_milli: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectedVisualEvidence {
    pub source: VisualPacketSource,
    pub rect: Rect,
    pub information_gain_milli: u16,
    pub confidence_milli: u16,
    pub relevance_milli: u16,
    pub normalized_cost_milli: u32,
    pub utility_score: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VisualPacketSelectionMode {
    MetadataOnly,
    Images,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualPacketSelection {
    pub mode: VisualPacketSelectionMode,
    pub selected: Vec<SelectedVisualEvidence>,
    pub dropped_candidates: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualPacketSelectionError {
    InvalidViewport,
    InvalidCandidateGeometry,
    InvalidCandidateScore,
}

impl fmt::Display for VisualPacketSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewport => write!(f, "visual packet viewport dimensions must be positive"),
            Self::InvalidCandidateGeometry => {
                write!(f, "visual packet candidate geometry is outside the viewport")
            }
            Self::InvalidCandidateScore => {
                write!(f, "visual packet candidate scores must be within 0..=1000")
            }
        }
    }
}

impl Error for VisualPacketSelectionError {}

#[derive(Debug, Clone)]
struct ScoredCandidate {
    selected: SelectedVisualEvidence,
}

pub fn select_visual_packet(
    viewport: (u32, u32),
    candidates: &[VisualPacketCandidate],
    budget: &VisualPacketBudget,
) -> Result<VisualPacketSelection, VisualPacketSelectionError> {
    if viewport.0 == 0 || viewport.1 == 0 {
        return Err(VisualPacketSelectionError::InvalidViewport);
    }

    let mut scored = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        validate_candidate(candidate, viewport)?;
        scored.push(score_candidate(candidate, viewport));
    }

    if budget.image_regions == 0 || scored.is_empty() {
        return Ok(VisualPacketSelection {
            mode: VisualPacketSelectionMode::MetadataOnly,
            selected: Vec::new(),
            dropped_candidates: candidates.len(),
        });
    }

    scored.sort_by(compare_scored_candidates);

    let mut selected = Vec::<SelectedVisualEvidence>::new();
    for candidate in scored {
        if selected.len() >= budget.image_regions {
            break;
        }
        if selected
            .iter()
            .any(|existing| evidence_is_redundant(existing, &candidate.selected))
        {
            continue;
        }
        selected.push(candidate.selected);
    }

    selected.sort_by(compare_selected_geometry);
    let dropped_candidates = candidates.len().saturating_sub(selected.len());
    let mode = if selected.is_empty() {
        VisualPacketSelectionMode::MetadataOnly
    } else {
        VisualPacketSelectionMode::Images
    };

    Ok(VisualPacketSelection {
        mode,
        selected,
        dropped_candidates,
    })
}

fn validate_candidate(
    candidate: &VisualPacketCandidate,
    viewport: (u32, u32),
) -> Result<(), VisualPacketSelectionError> {
    if candidate.information_gain_milli > 1000
        || candidate.confidence_milli > 1000
        || candidate.relevance_milli > 1000
    {
        return Err(VisualPacketSelectionError::InvalidCandidateScore);
    }

    let rect = &candidate.rect;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.width <= 0.0
        || rect.height <= 0.0
        || right > viewport.0 as f64
        || bottom > viewport.1 as f64
    {
        return Err(VisualPacketSelectionError::InvalidCandidateGeometry);
    }
    Ok(())
}

fn score_candidate(candidate: &VisualPacketCandidate, viewport: (u32, u32)) -> ScoredCandidate {
    let viewport_area = viewport.0 as f64 * viewport.1 as f64;
    let area = candidate.rect.width * candidate.rect.height;
    let area_cost_milli = ((area / viewport_area) * 1000.0).ceil().clamp(1.0, 1000.0) as u32;
    let normalized_cost_milli = source_base_cost(candidate.source) + area_cost_milli;

    let numerator = u128::from(candidate.information_gain_milli)
        * u128::from(candidate.confidence_milli)
        * u128::from(candidate.relevance_milli)
        * 1000;
    let utility_score = (numerator / u128::from(normalized_cost_milli.max(1)))
        .min(u128::from(u64::MAX)) as u64;

    ScoredCandidate {
        selected: SelectedVisualEvidence {
            source: candidate.source,
            rect: candidate.rect.clone(),
            information_gain_milli: candidate.information_gain_milli,
            confidence_milli: candidate.confidence_milli,
            relevance_milli: candidate.relevance_milli,
            normalized_cost_milli,
            utility_score,
        },
    }
}

fn source_base_cost(source: VisualPacketSource) -> u32 {
    match source {
        VisualPacketSource::ChangedRegion => 25,
        VisualPacketSource::ProgressiveElement => 50,
        VisualPacketSource::ProgressiveComponent => 100,
        VisualPacketSource::ProgressiveSection => 180,
        VisualPacketSource::ViewportFallback => 300,
    }
}

fn source_rank(source: VisualPacketSource) -> u8 {
    match source {
        VisualPacketSource::ChangedRegion => 0,
        VisualPacketSource::ProgressiveElement => 1,
        VisualPacketSource::ProgressiveComponent => 2,
        VisualPacketSource::ProgressiveSection => 3,
        VisualPacketSource::ViewportFallback => 4,
    }
}

fn compare_scored_candidates(left: &ScoredCandidate, right: &ScoredCandidate) -> Ordering {
    right
        .selected
        .utility_score
        .cmp(&left.selected.utility_score)
        .then_with(|| {
            right
                .selected
                .information_gain_milli
                .cmp(&left.selected.information_gain_milli)
        })
        .then_with(|| {
            right
                .selected
                .confidence_milli
                .cmp(&left.selected.confidence_milli)
        })
        .then_with(|| {
            right
                .selected
                .relevance_milli
                .cmp(&left.selected.relevance_milli)
        })
        .then_with(|| {
            left.selected
                .normalized_cost_milli
                .cmp(&right.selected.normalized_cost_milli)
        })
        .then_with(|| source_rank(left.selected.source).cmp(&source_rank(right.selected.source)))
        .then_with(|| compare_rect(&left.selected.rect, &right.selected.rect))
}

fn compare_selected_geometry(left: &SelectedVisualEvidence, right: &SelectedVisualEvidence) -> Ordering {
    compare_rect(&left.rect, &right.rect)
        .then_with(|| source_rank(left.source).cmp(&source_rank(right.source)))
}

fn compare_rect(left: &Rect, right: &Rect) -> Ordering {
    left.x
        .total_cmp(&right.x)
        .then_with(|| left.y.total_cmp(&right.y))
        .then_with(|| left.width.total_cmp(&right.width))
        .then_with(|| left.height.total_cmp(&right.height))
}

fn evidence_is_redundant(left: &SelectedVisualEvidence, right: &SelectedVisualEvidence) -> bool {
    overlap_fraction_of_smaller(&left.rect, &right.rect) >= 0.85
}

fn overlap_fraction_of_smaller(left: &Rect, right: &Rect) -> f64 {
    let left_right = left.x + left.width;
    let left_bottom = left.y + left.height;
    let right_right = right.x + right.width;
    let right_bottom = right.y + right.height;

    let overlap_width = left_right.min(right_right) - left.x.max(right.x);
    let overlap_height = left_bottom.min(right.bottom()) - left.y.max(right.y);
    if overlap_width <= 0.0 || overlap_height <= 0.0 {
        return 0.0;
    }

    let overlap_area = overlap_width * overlap_height;
    let smaller_area = (left.width * left.height).min(right.width * right.height);
    if smaller_area <= 0.0 {
        0.0
    } else {
        overlap_area / smaller_area
    }
}

fn summary_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .filter(|(k, _)| {
                    matches!(
                        k.as_str(),
                        "version"
                            | "route"
                            | "changed_refs"
                            | "removed_refs"
                            | "console_delta"
                            | "network_delta"
                            | "layout_changes"
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

fn trim_json(mut value: serde_json::Value, max_chars: usize) -> serde_json::Value {
    if let serde_json::Value::Object(map) = &mut value {
        for v in map.values_mut() {
            if let serde_json::Value::Array(a) = v {
                if a.len() > 24 {
                    a.truncate(24);
                }
            }
        }
    }
    let mut text = serde_json::to_string(&value).unwrap_or_default();
    if text.len() > max_chars {
        text.truncate(max_chars.saturating_sub(40));
        serde_json::json!({"truncated": true, "preview": text})
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(source: VisualPacketSource, rect: Rect, utility_score: u64) -> SelectedVisualEvidence {
        SelectedVisualEvidence {
            source,
            rect,
            information_gain_milli: 1000,
            confidence_milli: 1000,
            relevance_milli: 1000,
            normalized_cost_milli: 30,
            utility_score,
        }
    }

    #[test]
    fn token_estimate_is_bounded() {
        assert_eq!(approximate_tokens("12345678"), 2);
    }

    #[test]
    fn nested_context_is_redundant_when_it_fully_contains_local_evidence() {
        let local = selected(
            VisualPacketSource::ChangedRegion,
            Rect {
                x: 50.0,
                y: 50.0,
                width: 40.0,
                height: 20.0,
            },
            100,
        );
        let context = selected(
            VisualPacketSource::ProgressiveComponent,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 160.0,
            },
            50,
        );
        assert!(evidence_is_redundant(&local, &context));
    }
}
