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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FullPageOutputGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba_bytes: usize,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum FullPageError {
    #[error("full-page geometry is invalid")]
    InvalidGeometry,
    #[error("full-page policy is invalid")]
    InvalidPolicy,
    #[error("full-page document width does not match viewport width")]
    WidthMismatch,
    #[error("full-page document exceeds the CSS height budget")]
    DocumentTooTall,
    #[error("full-page tile budget exceeded")]
    TileBudgetExceeded,
    #[error("full-page output exceeds the RGBA memory budget")]
    OutputMemoryBudgetExceeded,
    #[error("full-page output exceeds the pixel-height budget")]
    OutputPixelHeightExceeded,
    #[error("full-page checked arithmetic overflow")]
    ArithmeticOverflow,
    #[error("full-page image buffer is invalid")]
    InvalidImage,
    #[error("full-page tile placement is outside the output")]
    PlacementOutOfBounds,
}

pub fn plan_full_page(
    document_css_width: f64,
    document_css_height: f64,
    viewport_css_width: f64,
    viewport_css_height: f64,
    original_scroll_y: f64,
    policy: FullPagePolicy,
) -> Result<FullPagePlan, FullPageError> {
    validate_policy(policy)?;
    if !document_css_width.is_finite()
        || !document_css_height.is_finite()
        || !viewport_css_width.is_finite()
        || !viewport_css_height.is_finite()
        || !original_scroll_y.is_finite()
        || document_css_width <= 0.0
        || document_css_height <= 0.0
        || viewport_css_width <= 0.0
        || viewport_css_height <= 0.0
        || original_scroll_y < 0.0
    {
        return Err(FullPageError::InvalidGeometry);
    }
    if document_css_width != viewport_css_width {
        return Err(FullPageError::WidthMismatch);
    }
    if document_css_height > policy.max_document_css_height {
        return Err(FullPageError::DocumentTooTall);
    }

    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    if original_scroll_y > max_scroll_y {
        return Err(FullPageError::InvalidGeometry);
    }

    let mut scroll_offsets_y = vec![0.0];
    if max_scroll_y > 0.0 {
        let mut next = viewport_css_height;
        while next < max_scroll_y {
            if scroll_offsets_y.len() >= policy.max_tiles {
                return Err(FullPageError::TileBudgetExceeded);
            }
            scroll_offsets_y.push(next);
            next += viewport_css_height;
            if !next.is_finite() {
                return Err(FullPageError::InvalidGeometry);
            }
        }
        if scroll_offsets_y.last().copied() != Some(max_scroll_y) {
            if scroll_offsets_y.len() >= policy.max_tiles {
                return Err(FullPageError::TileBudgetExceeded);
            }
            scroll_offsets_y.push(max_scroll_y);
        }
    }

    if scroll_offsets_y.len() > policy.max_tiles {
        return Err(FullPageError::TileBudgetExceeded);
    }
    if scroll_offsets_y
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(FullPageError::InvalidGeometry);
    }

    Ok(FullPagePlan {
        document_css_width,
        document_css_height,
        viewport_css_width,
        viewport_css_height,
        scroll_offsets_y,
    })
}

pub fn project_full_page_output(
    plan: &FullPagePlan,
    tile_pixel_width: u32,
    tile_pixel_height: u32,
    policy: FullPagePolicy,
) -> Result<FullPageOutputGeometry, FullPageError> {
    validate_policy(policy)?;
    validate_plan(plan, policy)?;
    if tile_pixel_width == 0 || tile_pixel_height == 0 {
        return Err(FullPageError::InvalidGeometry);
    }

    let scale_y = tile_pixel_height as f64 / plan.viewport_css_height;
    if !scale_y.is_finite() || scale_y <= 0.0 {
        return Err(FullPageError::InvalidGeometry);
    }
    let projected_height = (plan.document_css_height * scale_y).round();
    if !projected_height.is_finite() || projected_height <= 0.0 {
        return Err(FullPageError::InvalidGeometry);
    }
    if projected_height > u32::MAX as f64 {
        return Err(FullPageError::ArithmeticOverflow);
    }
    let pixel_height = projected_height as u32;
    if pixel_height > policy.max_output_pixel_height {
        return Err(FullPageError::OutputPixelHeightExceeded);
    }

    let rgba_bytes = usize::try_from(tile_pixel_width)
        .ok()
        .and_then(|width| {
            usize::try_from(pixel_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(FullPageError::ArithmeticOverflow)?;
    if rgba_bytes > policy.max_output_rgba_bytes {
        return Err(FullPageError::OutputMemoryBudgetExceeded);
    }

    Ok(FullPageOutputGeometry {
        pixel_width: tile_pixel_width,
        pixel_height,
        rgba_bytes,
    })
}

pub fn stitch_full_page_tile(
    output: &mut RgbaImage,
    viewport_css_height: f64,
    actual_scroll_y: f64,
    tile: &RgbaImage,
) -> Result<(), FullPageError> {
    output.validate().map_err(|_| FullPageError::InvalidImage)?;
    tile.validate().map_err(|_| FullPageError::InvalidImage)?;
    if output.width != tile.width {
        return Err(FullPageError::WidthMismatch);
    }
    if !viewport_css_height.is_finite()
        || viewport_css_height <= 0.0
        || !actual_scroll_y.is_finite()
        || actual_scroll_y < 0.0
    {
        return Err(FullPageError::InvalidGeometry);
    }

    let scale_y = tile.height as f64 / viewport_css_height;
    let top_px = (actual_scroll_y * scale_y).round();
    if !scale_y.is_finite() || scale_y <= 0.0 || !top_px.is_finite() || top_px < 0.0 {
        return Err(FullPageError::InvalidGeometry);
    }
    if top_px >= output.height as f64 {
        return Err(FullPageError::PlacementOutOfBounds);
    }
    if top_px > u32::MAX as f64 {
        return Err(FullPageError::ArithmeticOverflow);
    }
    let top_px = top_px as u32;
    let rows_to_copy = tile.height.min(output.height - top_px);

    let row_bytes = usize::try_from(output.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(FullPageError::ArithmeticOverflow)?;
    for row in 0..rows_to_copy {
        let source_start = usize::try_from(row)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageError::ArithmeticOverflow)?;
        let source_end = source_start
            .checked_add(row_bytes)
            .ok_or(FullPageError::ArithmeticOverflow)?;
        let destination_row = top_px
            .checked_add(row)
            .ok_or(FullPageError::ArithmeticOverflow)?;
        let destination_start = usize::try_from(destination_row)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageError::ArithmeticOverflow)?;
        let destination_end = destination_start
            .checked_add(row_bytes)
            .ok_or(FullPageError::ArithmeticOverflow)?;

        let source = tile
            .data
            .get(source_start..source_end)
            .ok_or(FullPageError::InvalidImage)?;
        let destination = output
            .data
            .get_mut(destination_start..destination_end)
            .ok_or(FullPageError::InvalidImage)?;
        destination.copy_from_slice(source);
    }

    Ok(())
}

fn validate_policy(policy: FullPagePolicy) -> Result<(), FullPageError> {
    if policy.max_tiles == 0
        || !policy.max_document_css_height.is_finite()
        || policy.max_document_css_height <= 0.0
        || policy.max_output_rgba_bytes == 0
        || policy.max_output_pixel_height == 0
    {
        return Err(FullPageError::InvalidPolicy);
    }
    Ok(())
}

fn validate_plan(plan: &FullPagePlan, policy: FullPagePolicy) -> Result<(), FullPageError> {
    if !plan.document_css_width.is_finite()
        || !plan.document_css_height.is_finite()
        || !plan.viewport_css_width.is_finite()
        || !plan.viewport_css_height.is_finite()
        || plan.document_css_width <= 0.0
        || plan.document_css_height <= 0.0
        || plan.viewport_css_width <= 0.0
        || plan.viewport_css_height <= 0.0
        || plan.scroll_offsets_y.is_empty()
    {
        return Err(FullPageError::InvalidGeometry);
    }
    if plan.document_css_width != plan.viewport_css_width {
        return Err(FullPageError::WidthMismatch);
    }
    if plan.document_css_height > policy.max_document_css_height {
        return Err(FullPageError::DocumentTooTall);
    }
    if plan.scroll_offsets_y.len() > policy.max_tiles {
        return Err(FullPageError::TileBudgetExceeded);
    }
    let max_scroll_y = (plan.document_css_height - plan.viewport_css_height).max(0.0);
    if plan.scroll_offsets_y.iter().any(|offset| {
        !offset.is_finite() || *offset < 0.0 || *offset > max_scroll_y
    }) || plan
        .scroll_offsets_y
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(FullPageError::InvalidGeometry);
    }
    Ok(())
}
