#![forbid(unsafe_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    pub max_frame_rgba_bytes: usize,
    pub gutter_rgba: [u8; 4],
}

impl Default for ContactSheetPolicy {
    fn default() -> Self {
        Self {
            gutter_px: DEFAULT_CONTACT_SHEET_GUTTER_PX,
            max_rgba_bytes: DEFAULT_MAX_CONTACT_SHEET_RGBA_BYTES,
            max_frame_rgba_bytes: DEFAULT_MAX_RESPONSIVE_FRAME_RGBA_BYTES,
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
    let mut placements = Vec::with_capacity(pixel_dimensions.len());

    for (index, ((pixel_w, pixel_h), (&preset, &viewport))) in pixel_dimensions
        .iter()
        .zip(plan.presets.iter().zip(plan.viewports.iter()))
        .enumerate()
    {
        let frame_bytes = checked_rgba_bytes(*pixel_w, *pixel_h)?;
        if frame_bytes > policy.max_frame_rgba_bytes {
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
            if expected > policy.max_frame_rgba_bytes {
                return Err(ResponsiveError::FrameMemoryBudgetExceeded);
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

#[async_trait]
pub trait LayoutProbe: Send + Sync {
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

    #[async_trait]
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
            max_frame_rgba_bytes: 512,
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
