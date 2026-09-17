use thiserror::Error;

use crate::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FullPagePolicy {
    pub max_tiles: usize,
    pub max_document_css_height: f64,
    pub max_output_rgba_bytes: usize,
    pub max_output_pixel_height: u32,
}

impl Default for FullPagePolicy {
    fn default() -> Self {
        Self {
            max_tiles: 32,
            max_document_css_height: 50_000.0,
            max_output_rgba_bytes: 128 * 1024 * 1024,
            max_output_pixel_height: 32_768,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullPagePlan {
    pub document_css_width: f64,
    pub document_css_height: f64,
    pub viewport_css_width: f64,
    pub viewport_css_height: f64,
    pub scroll_offsets_y: Vec<f64>,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum FullPagePlanError {
    #[error("full-page geometry is invalid")]
    InvalidGeometry,
    #[error("horizontal full-page stitching is unsupported")]
    HorizontalStitchingUnsupported,
    #[error("document exceeds the full-page CSS height budget")]
    DocumentTooTall,
    #[error("full-page plan exceeds the tile budget")]
    TooManyTiles,
    #[error("native scale is invalid")]
    InvalidScale,
    #[error("projected full-page output exceeds the pixel-height budget")]
    OutputTooTall,
    #[error("projected full-page output exceeds the RGBA byte budget")]
    OutputTooLarge,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum FullPageStitchError {
    #[error("full-page output or tile image is invalid")]
    InvalidImage,
    #[error("full-page tile width differs from output width")]
    WidthMismatch,
    #[error("full-page tile placement is invalid")]
    InvalidPlacement,
    #[error("full-page tile begins outside the output")]
    OutOfBounds,
    #[error("full-page row offset arithmetic overflowed")]
    ArithmeticOverflow,
}

pub fn plan_full_page(
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
    original_scroll_y: f64,
    policy: FullPagePolicy,
) -> Result<FullPagePlan, FullPagePlanError> {
    if !valid_positive(document_css_width)
        || !valid_positive(document_css_height)
        || !valid_positive(viewport_css_width)
        || !valid_positive(viewport_css_height)
        || !original_scroll_y.is_finite()
        || original_scroll_y < 0.0
        || policy.max_tiles == 0
        || !valid_positive(policy.max_document_css_height)
        || policy.max_output_rgba_bytes == 0
        || policy.max_output_pixel_height == 0
    {
        return Err(FullPagePlanError::InvalidGeometry);
    }

    if !approximately_equal(document_css_width, viewport_css_width) {
        return Err(FullPagePlanError::HorizontalStitchingUnsupported);
    }
    if document_css_height > policy.max_document_css_height {
        return Err(FullPagePlanError::DocumentTooTall);
    }

    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    if original_scroll_y > max_scroll_y + geometry_tolerance(max_scroll_y) {
        return Err(FullPagePlanError::InvalidGeometry);
    }

    let mut scroll_offsets_y = Vec::new();
    if max_scroll_y == 0.0 {
        scroll_offsets_y.push(0.0);
    } else {
        let mut offset = 0.0;
        while offset < max_scroll_y {
            scroll_offsets_y.push(offset);
            if scroll_offsets_y.len() > policy.max_tiles {
                return Err(FullPagePlanError::TooManyTiles);
            }
            offset += viewport_css_height;
            if !offset.is_finite() {
                return Err(FullPagePlanError::InvalidGeometry);
            }
        }
        if scroll_offsets_y
            .last()
            .is_none_or(|last| !approximately_equal(*last, max_scroll_y))
        {
            scroll_offsets_y.push(max_scroll_y);
        }
    }

    if scroll_offsets_y.len() > policy.max_tiles {
        return Err(FullPagePlanError::TooManyTiles);
    }

    Ok(FullPagePlan {
        document_css_width,
        document_css_height,
        viewport_css_width,
        viewport_css_height,
        scroll_offsets_y,
    })
}

pub fn project_output_height_px(
    document_css_height: f64,
    viewport_css_height: f64,
    tile_pixel_width: u32,
    tile_pixel_height: u32,
    policy: FullPagePolicy,
) -> Result<u32, FullPagePlanError> {
    if !valid_positive(document_css_height)
        || !valid_positive(viewport_css_height)
        || tile_pixel_width == 0
        || tile_pixel_height == 0
        || policy.max_output_rgba_bytes == 0
        || policy.max_output_pixel_height == 0
    {
        return Err(FullPagePlanError::InvalidGeometry);
    }

    let scale_y = tile_pixel_height as f64 / viewport_css_height;
    if !valid_positive(scale_y) {
        return Err(FullPagePlanError::InvalidScale);
    }
    let projected = (document_css_height * scale_y).round();
    if !projected.is_finite() || projected <= 0.0 || projected > u32::MAX as f64 {
        return Err(FullPagePlanError::OutputTooTall);
    }
    let output_height = projected as u32;
    if output_height > policy.max_output_pixel_height {
        return Err(FullPagePlanError::OutputTooTall);
    }

    let rgba_bytes = usize::try_from(tile_pixel_width)
        .ok()
        .and_then(|width| usize::try_from(output_height).ok().and_then(|height| width.checked_mul(height)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(FullPagePlanError::OutputTooLarge)?;
    if rgba_bytes > policy.max_output_rgba_bytes {
        return Err(FullPagePlanError::OutputTooLarge);
    }

    Ok(output_height)
}

pub fn scroll_tolerance_css(scale_y: f64) -> Result<f64, FullPagePlanError> {
    if !valid_positive(scale_y) {
        return Err(FullPagePlanError::InvalidScale);
    }
    Ok((1.0 / scale_y).max(0.25))
}

pub fn stitch_full_page_tile(
    output: &mut RgbaImage,
    actual_scroll_y: f64,
    scale_y: f64,
    tile: &RgbaImage,
) -> Result<(), FullPageStitchError> {
    output.validate().map_err(|_| FullPageStitchError::InvalidImage)?;
    tile.validate().map_err(|_| FullPageStitchError::InvalidImage)?;
    if output.width != tile.width {
        return Err(FullPageStitchError::WidthMismatch);
    }
    if !actual_scroll_y.is_finite() || actual_scroll_y < 0.0 || !valid_positive(scale_y) {
        return Err(FullPageStitchError::InvalidPlacement);
    }

    let top_px_f = (actual_scroll_y * scale_y).round();
    if !top_px_f.is_finite() || top_px_f < 0.0 || top_px_f > u32::MAX as f64 {
        return Err(FullPageStitchError::InvalidPlacement);
    }
    let top_px = top_px_f as u32;
    if top_px >= output.height {
        return Err(FullPageStitchError::OutOfBounds);
    }

    let rows_to_copy = tile.height.min(output.height - top_px);
    let row_bytes = usize::try_from(output.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(FullPageStitchError::ArithmeticOverflow)?;

    for row in 0..rows_to_copy {
        let source_start = usize::try_from(row)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageStitchError::ArithmeticOverflow)?;
        let source_end = source_start
            .checked_add(row_bytes)
            .ok_or(FullPageStitchError::ArithmeticOverflow)?;
        let output_row = top_px
            .checked_add(row)
            .ok_or(FullPageStitchError::ArithmeticOverflow)?;
        let dest_start = usize::try_from(output_row)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageStitchError::ArithmeticOverflow)?;
        let dest_end = dest_start
            .checked_add(row_bytes)
            .ok_or(FullPageStitchError::ArithmeticOverflow)?;
        output.data[dest_start..dest_end].copy_from_slice(&tile.data[source_start..source_end]);
    }

    Ok(())
}

fn valid_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn approximately_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= geometry_tolerance(left.abs().max(right.abs()))
}

fn geometry_tolerance(scale: f64) -> f64 {
    (scale * 1.0e-9).max(1.0e-9)
}
