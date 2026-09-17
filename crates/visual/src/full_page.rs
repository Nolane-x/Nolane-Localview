use crate::RgbaImage;
use thiserror::Error;

pub const MAX_FULL_PAGE_TILES: usize = 32;
pub const MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT: f64 = 50_000.0;
pub const MAX_FULL_PAGE_OUTPUT_RGBA_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT: u32 = 32_768;

const HORIZONTAL_GEOMETRY_TOLERANCE_CSS_PX: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FullPageError {
    #[error("full-page geometry is invalid")]
    InvalidGeometry,
    #[error("horizontal full-page stitching is not supported")]
    HorizontalOverflow,
    #[error("full-page document exceeds the CSS height bound")]
    DocumentTooTall,
    #[error("full-page tile count exceeds the bound")]
    TileLimitExceeded,
    #[error("full-page output pixel height exceeds the bound")]
    OutputPixelHeightExceeded,
    #[error("full-page output RGBA allocation exceeds the bound")]
    OutputMemoryBudgetExceeded,
    #[error("full-page image dimensions do not match")]
    DimensionMismatch,
    #[error("full-page RGBA buffer is invalid")]
    InvalidBuffer,
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

pub fn plan_full_page(
    document_css: (f64, f64),
    viewport_css: (f64, f64),
    original_scroll_y: f64,
) -> Result<FullPagePlan, FullPageError> {
    let (document_css_width, document_css_height) = document_css;
    let (viewport_css_width, viewport_css_height) = viewport_css;

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

    if document_css_height > MAX_FULL_PAGE_DOCUMENT_CSS_HEIGHT {
        return Err(FullPageError::DocumentTooTall);
    }

    if (document_css_width - viewport_css_width).abs() > HORIZONTAL_GEOMETRY_TOLERANCE_CSS_PX {
        return Err(FullPageError::HorizontalOverflow);
    }

    let max_scroll_y = (document_css_height - viewport_css_height).max(0.0);
    let mut scroll_offsets_y = vec![0.0];

    if max_scroll_y > 0.0 {
        let mut next = viewport_css_height;
        while next < max_scroll_y {
            if scroll_offsets_y.len() >= MAX_FULL_PAGE_TILES {
                return Err(FullPageError::TileLimitExceeded);
            }
            scroll_offsets_y.push(next);
            let advanced = next + viewport_css_height;
            if !advanced.is_finite() || advanced <= next {
                return Err(FullPageError::InvalidGeometry);
            }
            next = advanced;
        }

        let duplicate_final = scroll_offsets_y
            .last()
            .is_some_and(|last| (*last - max_scroll_y).abs() <= f64::EPSILON * max_scroll_y.max(1.0));
        if !duplicate_final {
            if scroll_offsets_y.len() >= MAX_FULL_PAGE_TILES {
                return Err(FullPageError::TileLimitExceeded);
            }
            scroll_offsets_y.push(max_scroll_y);
        }
    }

    if scroll_offsets_y.len() > MAX_FULL_PAGE_TILES
        || scroll_offsets_y
            .windows(2)
            .any(|pair| !pair[0].is_finite() || !pair[1].is_finite() || pair[1] <= pair[0])
    {
        return Err(FullPageError::TileLimitExceeded);
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
    native_viewport: (u32, u32),
) -> Result<FullPageOutputGeometry, FullPageError> {
    let (pixel_width, viewport_pixel_height) = native_viewport;
    if pixel_width == 0
        || viewport_pixel_height == 0
        || !plan.document_css_width.is_finite()
        || !plan.document_css_height.is_finite()
        || !plan.viewport_css_width.is_finite()
        || !plan.viewport_css_height.is_finite()
        || plan.document_css_width <= 0.0
        || plan.document_css_height <= 0.0
        || plan.viewport_css_width <= 0.0
        || plan.viewport_css_height <= 0.0
    {
        return Err(FullPageError::InvalidGeometry);
    }

    let scale_y = viewport_pixel_height as f64 / plan.viewport_css_height;
    let projected_height = (plan.document_css_height * scale_y).round();
    if !scale_y.is_finite()
        || scale_y <= 0.0
        || !projected_height.is_finite()
        || projected_height < 1.0
    {
        return Err(FullPageError::InvalidGeometry);
    }
    if projected_height > MAX_FULL_PAGE_OUTPUT_PIXEL_HEIGHT as f64 {
        return Err(FullPageError::OutputPixelHeightExceeded);
    }

    let pixel_height = projected_height as u32;
    let rgba_bytes = usize::try_from(pixel_width)
        .ok()
        .and_then(|width| {
            usize::try_from(pixel_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(FullPageError::OutputMemoryBudgetExceeded)?;
    if rgba_bytes > MAX_FULL_PAGE_OUTPUT_RGBA_BYTES {
        return Err(FullPageError::OutputMemoryBudgetExceeded);
    }

    Ok(FullPageOutputGeometry {
        pixel_width,
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
    output.validate().map_err(|_| FullPageError::InvalidBuffer)?;
    tile.validate().map_err(|_| FullPageError::InvalidBuffer)?;

    if output.width != tile.width {
        return Err(FullPageError::DimensionMismatch);
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
    if !scale_y.is_finite()
        || scale_y <= 0.0
        || !top_px.is_finite()
        || top_px < 0.0
        || top_px >= output.height as f64
    {
        return Err(FullPageError::InvalidGeometry);
    }

    let top_px = top_px as u32;
    let rows_to_copy = tile.height.min(output.height - top_px);
    if rows_to_copy == 0 {
        return Err(FullPageError::InvalidGeometry);
    }

    let row_bytes = usize::try_from(output.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(FullPageError::InvalidBuffer)?;
    for source_y in 0..rows_to_copy {
        let destination_y = top_px
            .checked_add(source_y)
            .ok_or(FullPageError::InvalidBuffer)?;
        let source_start = usize::try_from(source_y)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageError::InvalidBuffer)?;
        let source_end = source_start
            .checked_add(row_bytes)
            .ok_or(FullPageError::InvalidBuffer)?;
        let destination_start = usize::try_from(destination_y)
            .ok()
            .and_then(|row| row.checked_mul(row_bytes))
            .ok_or(FullPageError::InvalidBuffer)?;
        let destination_end = destination_start
            .checked_add(row_bytes)
            .ok_or(FullPageError::InvalidBuffer)?;

        let source = tile
            .data
            .get(source_start..source_end)
            .ok_or(FullPageError::InvalidBuffer)?;
        let destination = output
            .data
            .get_mut(destination_start..destination_end)
            .ok_or(FullPageError::InvalidBuffer)?;
        destination.copy_from_slice(source);
    }

    Ok(())
}
