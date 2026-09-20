#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

pub const DEFAULT_VIEWPORTS: &[Viewport] = &[
    Viewport { width: 320, height: 568 },
    Viewport { width: 360, height: 800 },
    Viewport { width: 375, height: 812 },
    Viewport { width: 390, height: 844 },
    Viewport { width: 430, height: 932 },
    Viewport { width: 768, height: 1024 },
    Viewport { width: 1024, height: 768 },
    Viewport { width: 1280, height: 720 },
    Viewport { width: 1440, height: 900 },
    Viewport { width: 1920, height: 1080 },
];

pub const MAX_CANONICAL_SWEEP_PRESETS: usize = 4;
pub const DEFAULT_CONTACT_SHEET_GUTTER_PX: u32 = 16;
pub const DEFAULT_MAX_CONTACT_SHEET_RGBA_BYTES: usize = 96 * 1024 * 1024;
pub const DEFAULT_MAX_RESPONSIVE_FRAME_RGBA_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_ADAPTIVE_PROBE_CAP: usize = 12;
pub const DEFAULT_ADAPTIVE_INITIAL_PROBE_CAP: usize = 6;
pub const DEFAULT_BREAKPOINT_TOLERANCE_PX: u32 = 16;
pub const DEFAULT_ADAPTIVE_MIN_WIDTH: u32 = 320;
pub const DEFAULT_ADAPTIVE_MAX_WIDTH: u32 = 1440;
pub const MAX_RESPONSIVE_OBSERVATION_NODES: usize = 256;
pub const MAX_RESPONSIVE_ISSUES: usize = 64;
const MAX_COLLISION_NODES: usize = 64;
const VIEWPORT_EDGE_TOLERANCE_PX: f64 = 1.0;
const PARENT_OVERFLOW_TOLERANCE_PX: f64 = 2.0;
const CONTROL_COLLISION_RATIO: f64 = 0.25;
const DRAMATIC_CENTER_SHIFT_RATIO: f64 = 0.30;
const DRAMATIC_AREA_RATIO: f64 = 3.0;
const NEARBY_WIDTH_DELTA_PX: u32 = 96;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponsivePresetId {
    MobileS,
    Mobile,
    Tablet,
    Desktop,
}

impl ResponsivePresetId {
    pub const fn viewport(self) -> Viewport {
        match self {
            Self::MobileS => Viewport { width: 320, height: 568 },
            Self::Mobile => Viewport { width: 390, height: 844 },
            Self::Tablet => Viewport { width: 768, height: 1024 },
            Self::Desktop => Viewport { width: 1440, height: 900 },
        }
    }

    const fn canonical_rank(self) -> u8 {
        match self {
            Self::MobileS => 0,
            Self::Mobile => 1,
            Self::Tablet => 2,
            Self::Desktop => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponsiveSweepPlan {
    pub presets: Vec<ResponsivePresetId>,
    pub viewports: Vec<Viewport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactSheetPolicy {
    pub gutter_px: u32,
    pub max_rgba_bytes: usize,
    pub max_frames_rgba_bytes: usize,
    pub gutter_rgba: [u8; 4],
}

impl Default for ContactSheetPolicy {
    fn default() -> Self {
        Self {
            gutter_px: DEFAULT_CONTACT_SHEET_GUTTER_PX,
            max_rgba_bytes: DEFAULT_MAX_CONTACT_SHEET_RGBA_BYTES,
            max_frames_rgba_bytes: DEFAULT_MAX_RESPONSIVE_FRAME_RGBA_BYTES,
            gutter_rgba: [10, 13, 18, 255],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactSheetPlacement {
    pub preset: ResponsivePresetId,
    pub viewport: Viewport,
    pub x: u32,
    pub y: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactSheetGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba_bytes: usize,
    pub placements: Vec<ContactSheetPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveFrame {
    pub preset: ResponsivePresetId,
    pub viewport: Viewport,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveContactSheet {
    pub geometry: ContactSheetGeometry,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsiveError {
    InvalidPresetCount,
    DuplicatePreset,
    FrameCountMismatch,
    FrameOrderMismatch,
    InvalidPixelGeometry,
    PixelArithmeticOverflow,
    FrameBufferLengthMismatch,
    FrameMemoryBudgetExceeded,
    ContactSheetMemoryBudgetExceeded,
    InvalidAdaptiveRange,
    InvalidAdaptiveProbeCap,
    InvalidResponsiveObservation,
}

impl fmt::Display for ResponsiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::InvalidPresetCount => "responsive_invalid_preset_count",
            Self::DuplicatePreset => "responsive_duplicate_preset",
            Self::FrameCountMismatch => "responsive_frame_count_mismatch",
            Self::FrameOrderMismatch => "responsive_frame_order_mismatch",
            Self::InvalidPixelGeometry => "responsive_invalid_pixel_geometry",
            Self::PixelArithmeticOverflow => "responsive_pixel_arithmetic_overflow",
            Self::FrameBufferLengthMismatch => "responsive_frame_buffer_length_mismatch",
            Self::FrameMemoryBudgetExceeded => "responsive_memory_budget_exceeded",
            Self::ContactSheetMemoryBudgetExceeded => {
                "responsive_contact_sheet_memory_budget_exceeded"
            }
            Self::InvalidAdaptiveRange => "responsive_invalid_adaptive_range",
            Self::InvalidAdaptiveProbeCap => "responsive_invalid_adaptive_probe_cap",
            Self::InvalidResponsiveObservation => "responsive_invalid_observation",
        };
        f.write_str(code)
    }
}

impl std::error::Error for ResponsiveError {}

pub fn plan_canonical_sweep(
    requested: &[ResponsivePresetId],
) -> Result<ResponsiveSweepPlan, ResponsiveError> {
    if requested.is_empty() || requested.len() > MAX_CANONICAL_SWEEP_PRESETS {
        return Err(ResponsiveError::InvalidPresetCount);
    }

    let mut seen = HashSet::with_capacity(requested.len());
    if requested.iter().copied().any(|preset| !seen.insert(preset)) {
        return Err(ResponsiveError::DuplicatePreset);
    }

    let mut presets = requested.to_vec();
    presets.sort_by_key(|preset| preset.canonical_rank());
    let viewports = presets.iter().copied().map(ResponsivePresetId::viewport).collect();

    Ok(ResponsiveSweepPlan { presets, viewports })
}

fn checked_rgba_bytes(width: u32, height: u32) -> Result<usize, ResponsiveError> {
    if width == 0 || height == 0 {
        return Err(ResponsiveError::InvalidPixelGeometry);
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
    pixels
        .checked_mul(4)
        .ok_or(ResponsiveError::PixelArithmeticOverflow)
}

pub fn project_contact_sheet(
    plan: &ResponsiveSweepPlan,
    pixel_dimensions: &[(u32, u32)],
    policy: ContactSheetPolicy,
) -> Result<ContactSheetGeometry, ResponsiveError> {
    if pixel_dimensions.len() != plan.presets.len() || plan.viewports.len() != plan.presets.len() {
        return Err(ResponsiveError::FrameCountMismatch);
    }

    let mut pixel_width = 0u32;
    let mut pixel_height = 0u32;
    let mut aggregate_frame_bytes = 0usize;
    let mut placements = Vec::with_capacity(pixel_dimensions.len());

    for (index, ((pixel_w, pixel_h), (&preset, &viewport))) in pixel_dimensions
        .iter()
        .zip(plan.presets.iter().zip(plan.viewports.iter()))
        .enumerate()
    {
        let frame_bytes = checked_rgba_bytes(*pixel_w, *pixel_h)?;
        aggregate_frame_bytes = aggregate_frame_bytes
            .checked_add(frame_bytes)
            .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
        if aggregate_frame_bytes > policy.max_frames_rgba_bytes {
            return Err(ResponsiveError::FrameMemoryBudgetExceeded);
        }

        pixel_width = pixel_width.max(*pixel_w);
        if index > 0 {
            pixel_height = pixel_height
                .checked_add(policy.gutter_px)
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
        }

        let y = pixel_height;
        pixel_height = pixel_height
            .checked_add(*pixel_h)
            .ok_or(ResponsiveError::PixelArithmeticOverflow)?;

        placements.push(ContactSheetPlacement {
            preset,
            viewport,
            x: 0,
            y,
            pixel_width: *pixel_w,
            pixel_height: *pixel_h,
        });
    }

    let rgba_bytes = checked_rgba_bytes(pixel_width, pixel_height)?;
    if rgba_bytes > policy.max_rgba_bytes {
        return Err(ResponsiveError::ContactSheetMemoryBudgetExceeded);
    }

    Ok(ContactSheetGeometry {
        pixel_width,
        pixel_height,
        rgba_bytes,
        placements,
    })
}

pub fn build_responsive_contact_sheet(
    plan: &ResponsiveSweepPlan,
    frames: &[ResponsiveFrame],
    policy: ContactSheetPolicy,
) -> Result<ResponsiveContactSheet, ResponsiveError> {
    if frames.len() != plan.presets.len() {
        return Err(ResponsiveError::FrameCountMismatch);
    }

    let dimensions = frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            if frame.preset != plan.presets[index] || frame.viewport != plan.viewports[index] {
                return Err(ResponsiveError::FrameOrderMismatch);
            }
            let expected = checked_rgba_bytes(frame.pixel_width, frame.pixel_height)?;
            if expected != frame.rgba.len() {
                return Err(ResponsiveError::FrameBufferLengthMismatch);
            }
            Ok((frame.pixel_width, frame.pixel_height))
        })
        .collect::<Result<Vec<_>, ResponsiveError>>()?;

    let geometry = project_contact_sheet(plan, &dimensions, policy)?;
    let mut rgba = vec![0u8; geometry.rgba_bytes];

    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&policy.gutter_rgba);
    }

    let destination_stride = (geometry.pixel_width as usize)
        .checked_mul(4)
        .ok_or(ResponsiveError::PixelArithmeticOverflow)?;

    for (frame, placement) in frames.iter().zip(geometry.placements.iter()) {
        let source_stride = (frame.pixel_width as usize)
            .checked_mul(4)
            .ok_or(ResponsiveError::PixelArithmeticOverflow)?;

        for row in 0..frame.pixel_height as usize {
            let source_start = row
                .checked_mul(source_stride)
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
            let source_end = source_start
                .checked_add(source_stride)
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
            let destination_row = (placement.y as usize)
                .checked_add(row)
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
            let destination_start = destination_row
                .checked_mul(destination_stride)
                .and_then(|offset| offset.checked_add(placement.x as usize * 4))
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;
            let destination_end = destination_start
                .checked_add(source_stride)
                .ok_or(ResponsiveError::PixelArithmeticOverflow)?;

            rgba[destination_start..destination_end]
                .copy_from_slice(&frame.rgba[source_start..source_end]);
        }
    }

    Ok(ResponsiveContactSheet { geometry, rgba })
}


#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponsiveDetectorState {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponsiveIssueKind {
    HorizontalOverflow,
    Clipping,
    UnexpectedDisappearance,
    ControlCollision,
    DramaticLayoutJump,
    TextOrControlOutsideViewport,
    BreakpointLocalRegression,
    NearbyWidthInstability,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponsiveIssueClass {
    Deterministic,
    Suspected,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsiveRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ResponsiveRect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn area(&self) -> f64 {
        self.width * self.height
    }

    fn valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsiveNodeObservation {
    pub reference: String,
    pub parent_reference: Option<String>,
    pub rect: ResponsiveRect,
    pub interactive: bool,
    pub text_or_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponsiveObservation {
    pub session: String,
    pub route: String,
    pub viewport: Viewport,
    pub snapshot_version: u64,
    pub complete: bool,
    pub nodes: Vec<ResponsiveNodeObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponsiveIssue {
    pub kind: ResponsiveIssueKind,
    pub session: String,
    pub route: String,
    pub viewport: Viewport,
    pub refs: Vec<String>,
    pub detector: String,
    pub evidence: Vec<String>,
    pub confidence_milli: u16,
    pub class: ResponsiveIssueClass,
    pub before_width: Option<u32>,
    pub after_width: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponsiveProbeEvaluation {
    pub state: ResponsiveDetectorState,
    pub issues: Vec<ResponsiveIssue>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponsiveProbeSample {
    pub width: u32,
    pub state: ResponsiveDetectorState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ObservedTransitionResolution {
    Resolved {
        detector: String,
        claim: String,
        lower_width: u32,
        upper_width: u32,
        lower_state: ResponsiveDetectorState,
        upper_state: ResponsiveDetectorState,
        tolerance_px: u32,
    },
    NoTransition {
        detector: String,
    },
    Inconclusive {
        detector: String,
        reason: String,
    },
}

pub fn bounded_adaptive_sweep(
    min: u32,
    max: u32,
    anchors: &[u32],
    initial_cap: usize,
) -> Result<Vec<u32>, ResponsiveError> {
    if min == 0 || max == 0 || min > max {
        return Err(ResponsiveError::InvalidAdaptiveRange);
    }
    if initial_cap < 2 || initial_cap > DEFAULT_ADAPTIVE_PROBE_CAP {
        return Err(ResponsiveError::InvalidAdaptiveProbeCap);
    }

    let mut widths = anchors
        .iter()
        .copied()
        .filter(|width| *width >= min && *width <= max)
        .collect::<Vec<_>>();
    widths.extend([min, max]);
    widths.sort_unstable();
    widths.dedup();

    if widths.len() > initial_cap {
        let last = widths.len() - 1;
        let mut selected = Vec::with_capacity(initial_cap);
        for slot in 0..initial_cap {
            let index = slot
                .checked_mul(last)
                .ok_or(ResponsiveError::InvalidAdaptiveProbeCap)?
                / (initial_cap - 1);
            selected.push(widths[index]);
        }
        selected.sort_unstable();
        selected.dedup();
        widths = selected;
    }

    while widths.len() < initial_cap {
        let Some((_, midpoint)) = widths
            .windows(2)
            .filter_map(|pair| {
                let gap = pair[1].saturating_sub(pair[0]);
                if gap <= 1 {
                    None
                } else {
                    Some((gap, pair[0] + gap / 2))
                }
            })
            .max_by_key(|(gap, midpoint)| (*gap, std::cmp::Reverse(*midpoint)))
        else {
            break;
        };
        if widths.binary_search(&midpoint).is_ok() {
            break;
        }
        widths.push(midpoint);
        widths.sort_unstable();
    }

    widths.sort_unstable();
    widths.dedup();
    Ok(widths)
}

fn issue_key(issue: &ResponsiveIssue) -> String {
    let mut refs = issue.refs.clone();
    refs.sort();
    format!(
        "{:?}|{}|{}|{}|{}|{:?}|{:?}",
        issue.kind,
        issue.session,
        issue.route,
        issue.viewport.width,
        refs.join(","),
        issue.before_width,
        issue.after_width
    )
}

pub fn deduplicate_responsive_issues(issues: Vec<ResponsiveIssue>) -> Vec<ResponsiveIssue> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for issue in issues {
        if out.len() >= MAX_RESPONSIVE_ISSUES {
            break;
        }
        if seen.insert(issue_key(&issue)) {
            out.push(issue);
        }
    }
    out
}

fn issue(
    observation: &ResponsiveObservation,
    kind: ResponsiveIssueKind,
    refs: Vec<String>,
    class: ResponsiveIssueClass,
    confidence_milli: u16,
    evidence: Vec<String>,
    before_width: Option<u32>,
    after_width: Option<u32>,
) -> ResponsiveIssue {
    let mut evidence_with_snapshot = Vec::with_capacity(evidence.len() + 1);
    evidence_with_snapshot.push(format!("snapshot_version={}", observation.snapshot_version));
    evidence_with_snapshot.extend(evidence);
    ResponsiveIssue {
        kind,
        session: observation.session.clone(),
        route: observation.route.clone(),
        viewport: observation.viewport,
        refs,
        detector: "responsive_geometry_v1".to_string(),
        evidence: evidence_with_snapshot,
        confidence_milli: confidence_milli.min(1000),
        class,
        before_width,
        after_width,
    }
}

fn overlap_area(a: &ResponsiveRect, b: &ResponsiveRect) -> f64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    (right - left).max(0.0) * (bottom - top).max(0.0)
}

fn direct_parent_child(a: &ResponsiveNodeObservation, b: &ResponsiveNodeObservation) -> bool {
    a.parent_reference.as_deref() == Some(b.reference.as_str())
        || b.parent_reference.as_deref() == Some(a.reference.as_str())
}

pub fn evaluate_responsive_observation(
    previous: Option<&ResponsiveObservation>,
    observation: &ResponsiveObservation,
) -> Result<ResponsiveProbeEvaluation, ResponsiveError> {
    if observation.session.is_empty()
        || observation.route.is_empty()
        || observation.viewport.width == 0
        || observation.viewport.height == 0
        || observation.nodes.len() > MAX_RESPONSIVE_OBSERVATION_NODES
        || observation
            .nodes
            .iter()
            .any(|node| node.reference.is_empty() || !node.rect.valid())
    {
        return Err(ResponsiveError::InvalidResponsiveObservation);
    }
    if let Some(previous) = previous {
        if previous.session != observation.session || previous.route != observation.route {
            return Err(ResponsiveError::InvalidResponsiveObservation);
        }
    }

    let viewport_width = f64::from(observation.viewport.width);
    let viewport_height = f64::from(observation.viewport.height);
    let nodes_by_ref = observation
        .nodes
        .iter()
        .map(|node| (node.reference.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut issues = Vec::new();
    let mut hard_failure = false;

    for node in &observation.nodes {
        let outside_x = node.rect.x < -VIEWPORT_EDGE_TOLERANCE_PX
            || node.rect.right() > viewport_width + VIEWPORT_EDGE_TOLERANCE_PX;
        let outside_y = node.rect.y < -VIEWPORT_EDGE_TOLERANCE_PX
            || node.rect.bottom() > viewport_height + VIEWPORT_EDGE_TOLERANCE_PX;
        if outside_x {
            hard_failure = true;
            issues.push(issue(
                observation,
                ResponsiveIssueKind::HorizontalOverflow,
                vec![node.reference.clone()],
                ResponsiveIssueClass::Deterministic,
                1000,
                vec![format!(
                    "rect_x={:.2};rect_right={:.2};viewport_width={}",
                    node.rect.x,
                    node.rect.right(),
                    observation.viewport.width
                )],
                None,
                None,
            ));
        }
        if node.text_or_control && (outside_x || outside_y) {
            hard_failure = true;
            issues.push(issue(
                observation,
                ResponsiveIssueKind::TextOrControlOutsideViewport,
                vec![node.reference.clone()],
                ResponsiveIssueClass::Deterministic,
                1000,
                vec![format!(
                    "rect=({:.2},{:.2},{:.2},{:.2});viewport={}x{}",
                    node.rect.x,
                    node.rect.y,
                    node.rect.width,
                    node.rect.height,
                    observation.viewport.width,
                    observation.viewport.height
                )],
                None,
                None,
            ));
        }
        if let Some(parent_ref) = node.parent_reference.as_deref() {
            if let Some(parent) = nodes_by_ref.get(parent_ref) {
                let exceeds_parent = node.rect.x < parent.rect.x - PARENT_OVERFLOW_TOLERANCE_PX
                    || node.rect.y < parent.rect.y - PARENT_OVERFLOW_TOLERANCE_PX
                    || node.rect.right() > parent.rect.right() + PARENT_OVERFLOW_TOLERANCE_PX
                    || node.rect.bottom() > parent.rect.bottom() + PARENT_OVERFLOW_TOLERANCE_PX;
                if exceeds_parent {
                    issues.push(issue(
                        observation,
                        ResponsiveIssueKind::Clipping,
                        vec![parent.reference.clone(), node.reference.clone()],
                        ResponsiveIssueClass::Suspected,
                        650,
                        vec![
                            "child_geometry_exceeds_parent_geometry".to_string(),
                            "computed_overflow_style_not_asserted".to_string(),
                        ],
                        None,
                        None,
                    ));
                }
            }
        }
    }

    let interactive = observation
        .nodes
        .iter()
        .filter(|node| node.interactive)
        .take(MAX_COLLISION_NODES)
        .collect::<Vec<_>>();
    for left_index in 0..interactive.len() {
        for right_index in (left_index + 1)..interactive.len() {
            let left = interactive[left_index];
            let right = interactive[right_index];
            if direct_parent_child(left, right) {
                continue;
            }
            let overlap = overlap_area(&left.rect, &right.rect);
            let smaller = left.rect.area().min(right.rect.area());
            if smaller > 0.0 && overlap / smaller >= CONTROL_COLLISION_RATIO {
                hard_failure = true;
                issues.push(issue(
                    observation,
                    ResponsiveIssueKind::ControlCollision,
                    vec![left.reference.clone(), right.reference.clone()],
                    ResponsiveIssueClass::Deterministic,
                    950,
                    vec![format!(
                        "overlap_ratio={:.3};threshold={:.3}",
                        overlap / smaller,
                        CONTROL_COLLISION_RATIO
                    )],
                    None,
                    None,
                ));
            }
        }
    }

    if let Some(previous) = previous {
        let width_delta = previous.viewport.width.abs_diff(observation.viewport.width);
        let previous_by_ref = previous
            .nodes
            .iter()
            .map(|node| (node.reference.as_str(), node))
            .collect::<BTreeMap<_, _>>();

        if width_delta <= NEARBY_WIDTH_DELTA_PX {
            for previous_node in &previous.nodes {
                if (previous_node.interactive || previous_node.text_or_control)
                    && !nodes_by_ref.contains_key(previous_node.reference.as_str())
                {
                    issues.push(issue(
                        observation,
                        ResponsiveIssueKind::UnexpectedDisappearance,
                        vec![previous_node.reference.clone()],
                        ResponsiveIssueClass::Suspected,
                        650,
                        vec!["stable_ref_missing_at_nearby_width".to_string()],
                        Some(previous.viewport.width),
                        Some(observation.viewport.width),
                    ));
                }
            }
        }

        for node in &observation.nodes {
            let Some(previous_node) = previous_by_ref.get(node.reference.as_str()) else {
                continue;
            };
            let previous_center_x =
                (previous_node.rect.x + previous_node.rect.width / 2.0)
                    / f64::from(previous.viewport.width);
            let previous_center_y =
                (previous_node.rect.y + previous_node.rect.height / 2.0)
                    / f64::from(previous.viewport.height);
            let center_x =
                (node.rect.x + node.rect.width / 2.0) / f64::from(observation.viewport.width);
            let center_y =
                (node.rect.y + node.rect.height / 2.0) / f64::from(observation.viewport.height);
            let center_shift =
                (center_x - previous_center_x).abs().max((center_y - previous_center_y).abs());
            let old_area = previous_node.rect.area();
            let new_area = node.rect.area();
            let area_ratio = if old_area > new_area {
                old_area / new_area.max(1.0)
            } else {
                new_area / old_area.max(1.0)
            };
            if width_delta <= NEARBY_WIDTH_DELTA_PX
                && (center_shift >= DRAMATIC_CENTER_SHIFT_RATIO
                    || area_ratio >= DRAMATIC_AREA_RATIO)
            {
                issues.push(issue(
                    observation,
                    ResponsiveIssueKind::DramaticLayoutJump,
                    vec![node.reference.clone()],
                    ResponsiveIssueClass::Deterministic,
                    900,
                    vec![format!(
                        "normalized_center_shift={center_shift:.3};area_ratio={area_ratio:.3}"
                    )],
                    Some(previous.viewport.width),
                    Some(observation.viewport.width),
                ));
            }
        }
    }

    let issues = deduplicate_responsive_issues(issues);
    Ok(ResponsiveProbeEvaluation {
        state: if !observation.complete {
            ResponsiveDetectorState::Inconclusive
        } else if hard_failure {
            ResponsiveDetectorState::Fail
        } else {
            ResponsiveDetectorState::Pass
        },
        issues,
    })
}

pub fn analyze_responsive_series(
    observations: &[ResponsiveObservation],
    evaluations: &[ResponsiveProbeEvaluation],
) -> Result<Vec<ResponsiveIssue>, ResponsiveError> {
    if observations.len() != evaluations.len() || observations.is_empty() {
        return Err(ResponsiveError::InvalidResponsiveObservation);
    }
    let mut indexed = observations
        .iter()
        .zip(evaluations.iter())
        .collect::<Vec<_>>();
    indexed.sort_by_key(|(observation, _)| observation.viewport.width);
    if indexed
        .windows(2)
        .any(|pair| pair[0].0.viewport.width == pair[1].0.viewport.width)
    {
        return Err(ResponsiveError::InvalidResponsiveObservation);
    }

    let mut issues = Vec::new();
    for triple in indexed.windows(3) {
        let left = triple[0];
        let middle = triple[1];
        let right = triple[2];
        if left.1.state == right.1.state
            && middle.1.state != left.1.state
            && !matches!(
                (left.1.state, middle.1.state, right.1.state),
                (
                    ResponsiveDetectorState::Inconclusive,
                    _,
                    _
                ) | (
                    _,
                    ResponsiveDetectorState::Inconclusive,
                    _
                ) | (
                    _,
                    _,
                    ResponsiveDetectorState::Inconclusive
                )
            )
        {
            issues.push(issue(
                middle.0,
                ResponsiveIssueKind::BreakpointLocalRegression,
                Vec::new(),
                ResponsiveIssueClass::Deterministic,
                1000,
                vec![format!(
                    "detector_state={:?}->{:?}->{:?}",
                    left.1.state, middle.1.state, right.1.state
                )],
                Some(left.0.viewport.width),
                Some(right.0.viewport.width),
            ));
        }
    }

    let states = indexed
        .iter()
        .map(|(observation, evaluation)| ResponsiveProbeSample {
            width: observation.viewport.width,
            state: evaluation.state,
        })
        .collect::<Vec<_>>();
    let transitions = states
        .windows(2)
        .filter(|pair| {
            pair[0].state != ResponsiveDetectorState::Inconclusive
                && pair[1].state != ResponsiveDetectorState::Inconclusive
                && pair[0].state != pair[1].state
        })
        .count();
    if transitions > 1 {
        if let Some((observation, _)) = indexed.get(indexed.len() / 2) {
            issues.push(issue(
                observation,
                ResponsiveIssueKind::NearbyWidthInstability,
                Vec::new(),
                ResponsiveIssueClass::Inconclusive,
                1000,
                vec![format!("detector_transitions={transitions}")],
                indexed.first().map(|entry| entry.0.viewport.width),
                indexed.last().map(|entry| entry.0.viewport.width),
            ));
        }
    }

    Ok(deduplicate_responsive_issues(issues))
}

pub fn resolve_observed_transition(
    samples: &[ResponsiveProbeSample],
    tolerance: u32,
    detector: impl Into<String>,
) -> ObservedTransitionResolution {
    let detector = detector.into();
    if samples.len() < 2 {
        return ObservedTransitionResolution::Inconclusive {
            detector,
            reason: "insufficient_probe_evidence".to_string(),
        };
    }

    let mut ordered = samples.to_vec();
    ordered.sort_by_key(|sample| sample.width);
    for pair in ordered.windows(2) {
        if pair[0].width == pair[1].width && pair[0].state != pair[1].state {
            return ObservedTransitionResolution::Inconclusive {
                detector,
                reason: "same_width_detector_instability".to_string(),
            };
        }
    }
    ordered.dedup_by_key(|sample| sample.width);
    if ordered
        .iter()
        .any(|sample| sample.state == ResponsiveDetectorState::Inconclusive)
    {
        return ObservedTransitionResolution::Inconclusive {
            detector,
            reason: "inconclusive_probe_evidence".to_string(),
        };
    }

    let transitions = ordered
        .windows(2)
        .filter(|pair| pair[0].state != pair[1].state)
        .collect::<Vec<_>>();
    match transitions.as_slice() {
        [] => ObservedTransitionResolution::NoTransition { detector },
        [pair] => {
            let gap = pair[1].width.saturating_sub(pair[0].width);
            if gap > tolerance.max(1) {
                ObservedTransitionResolution::Inconclusive {
                    detector,
                    reason: "transition_not_resolved_within_tolerance".to_string(),
                }
            } else {
                ObservedTransitionResolution::Resolved {
                    detector,
                    claim: "observed_responsive_transition".to_string(),
                    lower_width: pair[0].width,
                    upper_width: pair[1].width,
                    lower_state: pair[0].state,
                    upper_state: pair[1].state,
                    tolerance_px: tolerance.max(1),
                }
            }
        }
        _ => ObservedTransitionResolution::Inconclusive {
            detector,
            reason: "non_monotonic_detector".to_string(),
        },
    }
}

#[allow(async_fn_in_trait)]
pub trait LayoutProbe {
    async fn fails_at(&self, width: u32) -> bool;
}

pub async fn discover_breakpoint<P: LayoutProbe>(
    probe: &P,
    known_good: u32,
    known_bad: u32,
    tolerance: u32,
) -> Option<u32> {
    if known_good == known_bad {
        return None;
    }
    let (mut low, mut high) = if known_bad < known_good {
        (known_bad, known_good)
    } else {
        (known_good, known_bad)
    };
    let low_fails = probe.fails_at(low).await;
    let high_fails = probe.fails_at(high).await;
    if low_fails == high_fails {
        return None;
    }
    while high - low > tolerance.max(1) {
        let mid = low + (high - low) / 2;
        if probe.fails_at(mid).await == low_fails {
            low = mid;
        } else {
            high = mid;
        }
    }
    Some(if low_fails { high } else { low })
}

pub fn adaptive_sweep(min: u32, max: u32, anchors: &[u32]) -> Vec<u32> {
    let mut widths = anchors
        .iter()
        .copied()
        .filter(|width| *width >= min && *width <= max)
        .collect::<Vec<_>>();
    widths.extend([min, max]);
    widths.sort_unstable();
    widths.dedup();

    let mut extra = Vec::new();
    for pair in widths.windows(2) {
        if pair[1] - pair[0] > 160 {
            extra.push(pair[0] + (pair[1] - pair[0]) / 2);
        }
    }
    widths.extend(extra);
    widths.sort_unstable();
    widths.dedup();
    widths
}

#[cfg(test)]
mod tests {
    use super::*;

    struct P;

    impl LayoutProbe for P {
        async fn fails_at(&self, width: u32) -> bool {
            width < 728
        }
    }

    #[tokio::test]
    async fn finds_transition() {
        let breakpoint = discover_breakpoint(&P, 768, 700, 2).await.unwrap();
        assert!((727..=730).contains(&breakpoint));
    }

    #[test]
    fn contact_sheet_copies_exact_rows_and_keeps_opaque_gutters() {
        let plan = plan_canonical_sweep(&[
            ResponsivePresetId::MobileS,
            ResponsivePresetId::Mobile,
        ])
        .unwrap();
        let policy = ContactSheetPolicy {
            gutter_px: 1,
            max_rgba_bytes: 1024,
            max_frames_rgba_bytes: 512,
            gutter_rgba: [9, 8, 7, 255],
        };
        let frames = vec![
            ResponsiveFrame {
                preset: ResponsivePresetId::MobileS,
                viewport: ResponsivePresetId::MobileS.viewport(),
                pixel_width: 2,
                pixel_height: 1,
                rgba: vec![1, 2, 3, 255, 4, 5, 6, 255],
            },
            ResponsiveFrame {
                preset: ResponsivePresetId::Mobile,
                viewport: ResponsivePresetId::Mobile.viewport(),
                pixel_width: 1,
                pixel_height: 1,
                rgba: vec![10, 11, 12, 255],
            },
        ];

        let sheet = build_responsive_contact_sheet(&plan, &frames, policy).unwrap();
        assert_eq!(sheet.geometry.pixel_width, 2);
        assert_eq!(sheet.geometry.pixel_height, 3);
        assert_eq!(&sheet.rgba[0..8], &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(&sheet.rgba[8..16], &[9, 8, 7, 255, 9, 8, 7, 255]);
        assert_eq!(&sheet.rgba[16..20], &[10, 11, 12, 255]);
        assert_eq!(&sheet.rgba[20..24], &[9, 8, 7, 255]);
    }
}
