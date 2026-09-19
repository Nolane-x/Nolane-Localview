#![forbid(unsafe_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
    pub const ALL: [Self; 4] = [Self::MobileS, Self::Mobile, Self::Tablet, Self::Desktop];

    pub const fn viewport(self) -> Viewport {
        match self {
            Self::MobileS => Viewport { width: 320, height: 568 },
            Self::Mobile => Viewport { width: 390, height: 844 },
            Self::Tablet => Viewport { width: 768, height: 1024 },
            Self::Desktop => Viewport { width: 1440, height: 900 },
        }
    }

    const fn canonical_index(self) -> usize {
        match self {
            Self::MobileS => 0,
            Self::Mobile => 1,
            Self::Tablet => 2,
            Self::Desktop => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveSweepPlan {
    pub presets: Vec<ResponsivePresetId>,
    pub viewports: Vec<Viewport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactSheetPolicy {
    pub gutter_px: u32,
    pub max_rgba_bytes: usize,
}

impl Default for ContactSheetPolicy {
    fn default() -> Self {
        Self {
            gutter_px: 24,
            max_rgba_bytes: 96 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSheetPlacement {
    pub preset: ResponsivePresetId,
    pub viewport: Viewport,
    pub x: u32,
    pub y: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSheetGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba_bytes: usize,
    pub placements: Vec<ContactSheetPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponsiveError {
    InvalidPresetCount,
    DuplicatePreset,
    FrameCountMismatch,
    InvalidPixelGeometry,
    FrameViewportMismatch,
    InvalidRgbaBuffer,
    ArithmeticOverflow,
    ContactSheetMemoryBudgetExceeded,
}

impl std::fmt::Display for ResponsiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPresetCount => "responsive preset count must be between one and four",
            Self::DuplicatePreset => "responsive preset selection contains duplicates",
            Self::FrameCountMismatch => "responsive frame count differs from the planned sweep",
            Self::InvalidPixelGeometry => "responsive frame pixel geometry must be positive",
            Self::FrameViewportMismatch => "responsive frame viewport differs from the planned sweep",
            Self::InvalidRgbaBuffer => "responsive RGBA buffer does not match its pixel geometry",
            Self::ArithmeticOverflow => "responsive contact-sheet geometry overflowed",
            Self::ContactSheetMemoryBudgetExceeded => {
                "responsive contact sheet exceeds the bounded RGBA memory budget"
            }
        })
    }
}

impl std::error::Error for ResponsiveError {}

pub fn plan_canonical_sweep(
    requested: &[ResponsivePresetId],
) -> Result<ResponsiveSweepPlan, ResponsiveError> {
    if requested.is_empty() || requested.len() > ResponsivePresetId::ALL.len() {
        return Err(ResponsiveError::InvalidPresetCount);
    }

    let mut present = [false; 4];
    for preset in requested {
        let index = preset.canonical_index();
        if present[index] {
            return Err(ResponsiveError::DuplicatePreset);
        }
        present[index] = true;
    }

    let presets = ResponsivePresetId::ALL
        .into_iter()
        .filter(|preset| present[preset.canonical_index()])
        .collect::<Vec<_>>();
    let viewports = presets.iter().map(|preset| preset.viewport()).collect();

    Ok(ResponsiveSweepPlan { presets, viewports })
}

pub fn project_contact_sheet(
    plan: &ResponsiveSweepPlan,
    frame_pixels: &[(u32, u32)],
    policy: ContactSheetPolicy,
) -> Result<ContactSheetGeometry, ResponsiveError> {
    if frame_pixels.len() != plan.presets.len() || plan.viewports.len() != plan.presets.len() {
        return Err(ResponsiveError::FrameCountMismatch);
    }

    let mut pixel_width = 0_u32;
    let mut pixel_height = 0_u32;
    let mut placements = Vec::with_capacity(frame_pixels.len());

    for (index, ((pixel_frame_width, pixel_frame_height), preset)) in frame_pixels
        .iter()
        .copied()
        .zip(plan.presets.iter().copied())
        .enumerate()
    {
        if pixel_frame_width == 0 || pixel_frame_height == 0 {
            return Err(ResponsiveError::InvalidPixelGeometry);
        }

        if index > 0 {
            pixel_height = pixel_height
                .checked_add(policy.gutter_px)
                .ok_or(ResponsiveError::ArithmeticOverflow)?;
        }

        let y = pixel_height;
        pixel_height = pixel_height
            .checked_add(pixel_frame_height)
            .ok_or(ResponsiveError::ArithmeticOverflow)?;
        pixel_width = pixel_width.max(pixel_frame_width);

        placements.push(ContactSheetPlacement {
            preset,
            viewport: preset.viewport(),
            x: 0,
            y,
            pixel_width: pixel_frame_width,
            pixel_height: pixel_frame_height,
        });
    }

    let rgba_bytes_u64 = u64::from(pixel_width)
        .checked_mul(u64::from(pixel_height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ResponsiveError::ArithmeticOverflow)?;
    let rgba_bytes =
        usize::try_from(rgba_bytes_u64).map_err(|_| ResponsiveError::ArithmeticOverflow)?;

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

#[derive(Debug, Clone, Copy)]
pub struct ResponsiveRgbaFrame<'a> {
    pub preset: ResponsivePresetId,
    pub viewport: Viewport,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveContactSheet {
    pub geometry: ContactSheetGeometry,
    pub rgba: Vec<u8>,
}

pub fn build_responsive_contact_sheet(
    plan: &ResponsiveSweepPlan,
    frames: &[ResponsiveRgbaFrame<'_>],
    policy: ContactSheetPolicy,
) -> Result<ResponsiveContactSheet, ResponsiveError> {
    if frames.len() != plan.presets.len() || plan.viewports.len() != plan.presets.len() {
        return Err(ResponsiveError::FrameCountMismatch);
    }

    for (index, frame) in frames.iter().enumerate() {
        if frame.preset != plan.presets[index] || frame.viewport != plan.viewports[index] {
            return Err(ResponsiveError::FrameViewportMismatch);
        }

        if frame.pixel_width == 0 || frame.pixel_height == 0 {
            return Err(ResponsiveError::InvalidPixelGeometry);
        }

        let expected_len = rgba_len(frame.pixel_width, frame.pixel_height)?;
        if frame.rgba.len() != expected_len {
            return Err(ResponsiveError::InvalidRgbaBuffer);
        }
    }

    let frame_pixels = frames
        .iter()
        .map(|frame| (frame.pixel_width, frame.pixel_height))
        .collect::<Vec<_>>();
    let geometry = project_contact_sheet(plan, &frame_pixels, policy)?;

    // Opaque neutral background. Source frames are already private-redacted;
    // contact-sheet construction never rescales or crops them.
    let mut rgba = vec![0_u8; geometry.rgba_bytes];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[32, 33, 36, 255]);
    }

    let sheet_row_bytes = usize::try_from(geometry.pixel_width)
        .map_err(|_| ResponsiveError::ArithmeticOverflow)?
        .checked_mul(4)
        .ok_or(ResponsiveError::ArithmeticOverflow)?;

    for (frame, placement) in frames.iter().zip(&geometry.placements) {
        let source_row_bytes = usize::try_from(frame.pixel_width)
            .map_err(|_| ResponsiveError::ArithmeticOverflow)?
            .checked_mul(4)
            .ok_or(ResponsiveError::ArithmeticOverflow)?;
        let placement_y =
            usize::try_from(placement.y).map_err(|_| ResponsiveError::ArithmeticOverflow)?;

        for row in 0..usize::try_from(frame.pixel_height)
            .map_err(|_| ResponsiveError::ArithmeticOverflow)?
        {
            let source_start = row
                .checked_mul(source_row_bytes)
                .ok_or(ResponsiveError::ArithmeticOverflow)?;
            let source_end = source_start
                .checked_add(source_row_bytes)
                .ok_or(ResponsiveError::ArithmeticOverflow)?;

            let destination_row = placement_y
                .checked_add(row)
                .ok_or(ResponsiveError::ArithmeticOverflow)?;
            let destination_start = destination_row
                .checked_mul(sheet_row_bytes)
                .and_then(|offset| {
                    usize::try_from(placement.x)
                        .ok()
                        .and_then(|x| x.checked_mul(4))
                        .and_then(|x_bytes| offset.checked_add(x_bytes))
                })
                .ok_or(ResponsiveError::ArithmeticOverflow)?;
            let destination_end = destination_start
                .checked_add(source_row_bytes)
                .ok_or(ResponsiveError::ArithmeticOverflow)?;

            rgba.get_mut(destination_start..destination_end)
                .ok_or(ResponsiveError::ArithmeticOverflow)?
                .copy_from_slice(
                    frame
                        .rgba
                        .get(source_start..source_end)
                        .ok_or(ResponsiveError::InvalidRgbaBuffer)?,
                );
        }
    }

    Ok(ResponsiveContactSheet { geometry, rgba })
}

fn rgba_len(width: u32, height: u32) -> Result<usize, ResponsiveError> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ResponsiveError::ArithmeticOverflow)?;
    usize::try_from(bytes).map_err(|_| ResponsiveError::ArithmeticOverflow)
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
            low = mid
        } else {
            high = mid
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
}
