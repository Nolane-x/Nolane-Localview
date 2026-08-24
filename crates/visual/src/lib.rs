#![forbid(unsafe_code)]

use std::io::{BufReader, Cursor};

use localview_protocol::Rect;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_ENCODED_PNG_BYTES: usize = 24 * 1024 * 1024;
const MAX_DECODED_IMAGE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum VisualError {
    #[error("image dimensions do not match buffer length")]
    InvalidBuffer,
    #[error("images have different dimensions")]
    DimensionMismatch,
    #[error("CSS viewport dimensions are invalid")]
    InvalidViewport,
    #[error("mask geometry is invalid")]
    InvalidMaskGeometry,
    #[error("capture region geometry is invalid")]
    InvalidRegionGeometry,
    #[error("changed-region policy is invalid")]
    InvalidChangePolicy,
    #[error("PNG input exceeds the encoded byte budget")]
    EncodedPngTooLarge,
    #[error("decoded PNG exceeds the image byte budget")]
    DecodedImageTooLarge,
    #[error("PNG decoder returned an unsupported output format")]
    UnsupportedPngOutput,
    #[error("PNG decode failed: {0}")]
    PngDecode(String),
    #[error("PNG encode failed: {0}")]
    PngEncode(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn validate(&self) -> Result<(), VisualError> {
        let expected = self
            .width
            .checked_mul(self.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or(VisualError::InvalidBuffer)?;
        if self.width == 0 || self.height == 0 || self.data.len() != expected {
            return Err(VisualError::InvalidBuffer);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub changed_pixels: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualDiff {
    pub changed_pixels: u64,
    pub changed_ratio: f64,
    pub bounding_box: Option<DiffRegion>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ChangedRegionPolicy {
    pub tile_px: u32,
    pub threshold: u8,
    pub max_regions: usize,
    pub viewport_fallback_ratio: f64,
}

impl Default for ChangedRegionPolicy {
    fn default() -> Self {
        Self {
            tile_px: 64,
            threshold: 8,
            max_regions: 8,
            viewport_fallback_ratio: 0.35,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangedRegionPlan {
    Unchanged,
    Regions {
        regions: Vec<Rect>,
        changed_ratio: f64,
    },
    Viewport {
        changed_ratio: f64,
    },
}

pub fn pixel_diff(
    before: &RgbaImage,
    after: &RgbaImage,
    threshold: u8,
) -> Result<VisualDiff, VisualError> {
    before.validate()?;
    after.validate()?;
    if before.width != after.width || before.height != after.height {
        return Err(VisualError::DimensionMismatch);
    }
    let mut changed = 0u64;
    let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    for p in 0..(before.width * before.height) as usize {
        let i = p * 4;
        let delta = (0..4)
            .map(|c| before.data[i + c].abs_diff(after.data[i + c]))
            .max()
            .unwrap_or(0);
        if delta > threshold {
            changed += 1;
            let x = p as u32 % before.width;
            let y = p as u32 / before.width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let bounding_box = (changed > 0).then(|| DiffRegion {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
        changed_pixels: changed,
    });
    Ok(VisualDiff {
        changed_pixels: changed,
        changed_ratio: changed as f64 / (before.width as f64 * before.height as f64).max(1.0),
        bounding_box,
    })
}

pub fn changed_tiles(
    before: &RgbaImage,
    after: &RgbaImage,
    tile: u32,
    threshold: u8,
) -> Result<Vec<DiffRegion>, VisualError> {
    before.validate()?;
    after.validate()?;
    if before.width != after.width || before.height != after.height {
        return Err(VisualError::DimensionMismatch);
    }
    let tile = tile.max(1);
    let mut regions = Vec::new();
    let mut y = 0;
    while y < before.height {
        let mut x = 0;
        while x < before.width {
            let w = tile.min(before.width - x);
            let h = tile.min(before.height - y);
            let mut changed = 0;
            for yy in y..y + h {
                for xx in x..x + w {
                    let i = ((yy * before.width + xx) * 4) as usize;
                    let delta = (0..4)
                        .map(|c| before.data[i + c].abs_diff(after.data[i + c]))
                        .max()
                        .unwrap_or(0);
                    if delta > threshold {
                        changed += 1;
                    }
                }
            }
            if changed > 0 {
                regions.push(DiffRegion {
                    x,
                    y,
                    width: w,
                    height: h,
                    changed_pixels: changed,
                });
            }
            x += tile;
        }
        y += tile;
    }
    Ok(regions)
}

pub fn plan_changed_css_regions(
    before: &RgbaImage,
    after: &RgbaImage,
    css_viewport: (f64, f64),
    policy: ChangedRegionPolicy,
) -> Result<ChangedRegionPlan, VisualError> {
    before.validate()?;
    after.validate()?;
    if before.width != after.width || before.height != after.height {
        return Err(VisualError::DimensionMismatch);
    }

    let (css_width, css_height) = css_viewport;
    if !css_width.is_finite()
        || !css_height.is_finite()
        || css_width <= 0.0
        || css_height <= 0.0
    {
        return Err(VisualError::InvalidViewport);
    }
    if policy.tile_px == 0
        || policy.max_regions == 0
        || !policy.viewport_fallback_ratio.is_finite()
        || policy.viewport_fallback_ratio <= 0.0
        || policy.viewport_fallback_ratio > 1.0
    {
        return Err(VisualError::InvalidChangePolicy);
    }

    let diff = pixel_diff(before, after, policy.threshold)?;
    if diff.changed_pixels == 0 {
        return Ok(ChangedRegionPlan::Unchanged);
    }
    if diff.changed_ratio >= policy.viewport_fallback_ratio {
        return Ok(ChangedRegionPlan::Viewport {
            changed_ratio: diff.changed_ratio,
        });
    }

    let tiles = changed_tiles(before, after, policy.tile_px, policy.threshold)?;
    let regions = coalesce_regions(tiles);
    if regions.len() > policy.max_regions {
        return Ok(ChangedRegionPlan::Viewport {
            changed_ratio: diff.changed_ratio,
        });
    }

    let scale_x = css_width / before.width as f64;
    let scale_y = css_height / before.height as f64;
    let regions = regions
        .into_iter()
        .map(|region| Rect {
            x: region.x as f64 * scale_x,
            y: region.y as f64 * scale_y,
            width: region.width as f64 * scale_x,
            height: region.height as f64 * scale_y,
        })
        .collect();

    Ok(ChangedRegionPlan::Regions {
        regions,
        changed_ratio: diff.changed_ratio,
    })
}

fn coalesce_regions(mut pending: Vec<DiffRegion>) -> Vec<DiffRegion> {
    pending.sort_by_key(|region| (region.y, region.x));
    let mut merged = Vec::<DiffRegion>::new();

    while let Some(mut current) = pending.pop() {
        while let Some(index) = pending
            .iter()
            .position(|candidate| regions_overlap_or_edge_touch(&current, candidate))
        {
            let other = pending.swap_remove(index);
            current = merge_regions(current, other);
        }
        merged.push(current);
    }

    merged.sort_by_key(|region| (region.y, region.x));
    merged
}

fn regions_overlap_or_edge_touch(left: &DiffRegion, right: &DiffRegion) -> bool {
    let left_right = left.x.saturating_add(left.width);
    let left_bottom = left.y.saturating_add(left.height);
    let right_right = right.x.saturating_add(right.width);
    let right_bottom = right.y.saturating_add(right.height);

    let x_overlap = left.x < right_right && right.x < left_right;
    let y_overlap = left.y < right_bottom && right.y < left_bottom;
    let x_touch = left_right == right.x || right_right == left.x;
    let y_touch = left_bottom == right.y || right_bottom == left.y;

    (x_overlap && y_overlap) || (x_touch && y_overlap) || (y_touch && x_overlap)
}

fn merge_regions(left: DiffRegion, right: DiffRegion) -> DiffRegion {
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    let right_edge = left
        .x
        .saturating_add(left.width)
        .max(right.x.saturating_add(right.width));
    let bottom_edge = left
        .y
        .saturating_add(left.height)
        .max(right.y.saturating_add(right.height));
    DiffRegion {
        x,
        y,
        width: right_edge.saturating_sub(x),
        height: bottom_edge.saturating_sub(y),
        changed_pixels: left.changed_pixels.saturating_add(right.changed_pixels),
    }
}

/// Decodes one PNG frame into a bounded RGBA8 image.
pub fn decode_png_rgba(bytes: &[u8]) -> Result<RgbaImage, VisualError> {
    if bytes.len() > MAX_ENCODED_PNG_BYTES {
        return Err(VisualError::EncodedPngTooLarge);
    }

    let limits = png::Limits {
        bytes: MAX_DECODED_IMAGE_BYTES,
    };
    let source = BufReader::new(Cursor::new(bytes));
    let mut decoder = png::Decoder::new_with_limits(source, limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|error| VisualError::PngDecode(error.to_string()))?;
    let output_size = reader
        .output_buffer_size()
        .ok_or(VisualError::DecodedImageTooLarge)?;
    if output_size > MAX_DECODED_IMAGE_BYTES {
        return Err(VisualError::DecodedImageTooLarge);
    }

    let mut buffer = vec![0; output_size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| VisualError::PngDecode(error.to_string()))?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(VisualError::UnsupportedPngOutput);
    }
    let frame = &buffer[..info.buffer_size()];
    let pixels = usize::try_from(info.width)
        .ok()
        .and_then(|width| {
            usize::try_from(info.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(VisualError::DecodedImageTooLarge)?;
    let rgba_len = pixels
        .checked_mul(4)
        .ok_or(VisualError::DecodedImageTooLarge)?;
    if rgba_len > MAX_DECODED_IMAGE_BYTES {
        return Err(VisualError::DecodedImageTooLarge);
    }

    let mut rgba = Vec::with_capacity(rgba_len);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(frame),
        png::ColorType::Rgb => {
            for pixel in frame.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &gray in frame {
                rgba.extend_from_slice(&[gray, gray, gray, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in frame.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => return Err(VisualError::UnsupportedPngOutput),
    }
    if rgba.len() != rgba_len {
        return Err(VisualError::UnsupportedPngOutput);
    }

    let image = RgbaImage {
        width: info.width,
        height: info.height,
        data: rgba,
    };
    image.validate()?;
    Ok(image)
}

/// Encodes a bounded RGBA8 image as PNG.
pub fn encode_png_rgba(image: &RgbaImage) -> Result<Vec<u8>, VisualError> {
    image.validate()?;
    if image.data.len() > MAX_DECODED_IMAGE_BYTES {
        return Err(VisualError::DecodedImageTooLarge);
    }

    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, image.width, image.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| VisualError::PngEncode(error.to_string()))?;
        writer
            .write_image_data(&image.data)
            .map_err(|error| VisualError::PngEncode(error.to_string()))?;
    }
    if encoded.len() > MAX_ENCODED_PNG_BYTES {
        return Err(VisualError::EncodedPngTooLarge);
    }
    Ok(encoded)
}

/// Crops one viewport-relative CSS rectangle from a native PNG and re-encodes
/// it as a bounded PNG. The source dimensions must exactly match the native
/// capture metadata so CSS-to-device scaling cannot silently drift.
pub fn crop_png_css_rect(
    png: &[u8],
    expected_dimensions: (u32, u32),
    css_viewport: (f64, f64),
    rect: &Rect,
) -> Result<Vec<u8>, VisualError> {
    let image = decode_png_rgba(png)?;
    if (image.width, image.height) != expected_dimensions {
        return Err(VisualError::DimensionMismatch);
    }

    let (css_width, css_height) = css_viewport;
    if !css_width.is_finite()
        || !css_height.is_finite()
        || css_width <= 0.0
        || css_height <= 0.0
    {
        return Err(VisualError::InvalidViewport);
    }

    let rect_right = rect.x + rect.width;
    let rect_bottom = rect.y + rect.height;
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || !rect_right.is_finite()
        || !rect_bottom.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return Err(VisualError::InvalidRegionGeometry);
    }

    let css_left = rect.x.max(0.0).min(css_width);
    let css_top = rect.y.max(0.0).min(css_height);
    let css_right = rect_right.max(0.0).min(css_width);
    let css_bottom = rect_bottom.max(0.0).min(css_height);
    if css_right <= css_left || css_bottom <= css_top {
        return Err(VisualError::InvalidRegionGeometry);
    }

    let scale_x = image.width as f64 / css_width;
    let scale_y = image.height as f64 / css_height;
    let left = (css_left * scale_x).floor().max(0.0) as u32;
    let top = (css_top * scale_y).floor().max(0.0) as u32;
    let right = (css_right * scale_x)
        .ceil()
        .min(image.width as f64) as u32;
    let bottom = (css_bottom * scale_y)
        .ceil()
        .min(image.height as f64) as u32;
    if right <= left || bottom <= top {
        return Err(VisualError::InvalidRegionGeometry);
    }

    let width = right - left;
    let height = bottom - top;
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(VisualError::DecodedImageTooLarge)?;
    let output_bytes = row_bytes
        .checked_mul(usize::try_from(height).map_err(|_| VisualError::DecodedImageTooLarge)?)
        .ok_or(VisualError::DecodedImageTooLarge)?;
    if output_bytes > MAX_DECODED_IMAGE_BYTES {
        return Err(VisualError::DecodedImageTooLarge);
    }

    let source_row_bytes = usize::try_from(image.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or(VisualError::InvalidBuffer)?;
    let left_bytes = usize::try_from(left)
        .ok()
        .and_then(|left| left.checked_mul(4))
        .ok_or(VisualError::InvalidBuffer)?;
    let mut data = Vec::with_capacity(output_bytes);
    for y in top..bottom {
        let row_start = usize::try_from(y)
            .ok()
            .and_then(|y| y.checked_mul(source_row_bytes))
            .and_then(|offset| offset.checked_add(left_bytes))
            .ok_or(VisualError::InvalidBuffer)?;
        let row_end = row_start
            .checked_add(row_bytes)
            .ok_or(VisualError::InvalidBuffer)?;
        let row = image
            .data
            .get(row_start..row_end)
            .ok_or(VisualError::InvalidBuffer)?;
        data.extend_from_slice(row);
    }

    encode_png_rgba(&RgbaImage {
        width,
        height,
        data,
    })
}

/// Redacts viewport-relative CSS rectangles in native PNG pixels and re-encodes
/// the frame. The decoded dimensions must exactly match native capture metadata.
pub fn redact_png_css_rects(
    png: &[u8],
    expected_dimensions: (u32, u32),
    css_viewport: (f64, f64),
    rects: &[Rect],
) -> Result<(Vec<u8>, usize), VisualError> {
    let mut image = decode_png_rgba(png)?;
    if (image.width, image.height) != expected_dimensions {
        return Err(VisualError::DimensionMismatch);
    }
    let applied = redact_css_rects(&mut image, css_viewport, rects)?;
    let encoded = encode_png_rgba(&image)?;
    Ok((encoded, applied))
}

/// Redacts viewport-relative CSS rectangles directly in an RGBA image.
///
/// Rectangle coordinates are expressed in CSS pixels. The function first
/// validates the entire mask set, then scales it to native pixel dimensions,
/// clamps it to the captured frame, and fills every affected pixel with opaque
/// black. Validation happens before mutation so malformed geometry fails closed
/// without leaving a partially redacted frame.
pub fn redact_css_rects(
    image: &mut RgbaImage,
    css_viewport: (f64, f64),
    rects: &[Rect],
) -> Result<usize, VisualError> {
    image.validate()?;

    let (css_width, css_height) = css_viewport;
    if !css_width.is_finite()
        || !css_height.is_finite()
        || css_width <= 0.0
        || css_height <= 0.0
    {
        return Err(VisualError::InvalidViewport);
    }

    for rect in rects {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        if !rect.x.is_finite()
            || !rect.y.is_finite()
            || !rect.width.is_finite()
            || !rect.height.is_finite()
            || !right.is_finite()
            || !bottom.is_finite()
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            return Err(VisualError::InvalidMaskGeometry);
        }
    }

    let scale_x = image.width as f64 / css_width;
    let scale_y = image.height as f64 / css_height;
    let mut applied = 0usize;

    for rect in rects {
        let css_left = rect.x.max(0.0).min(css_width);
        let css_top = rect.y.max(0.0).min(css_height);
        let css_right = (rect.x + rect.width).max(0.0).min(css_width);
        let css_bottom = (rect.y + rect.height).max(0.0).min(css_height);
        if css_right <= css_left || css_bottom <= css_top {
            continue;
        }

        let left = (css_left * scale_x).floor().max(0.0) as u32;
        let top = (css_top * scale_y).floor().max(0.0) as u32;
        let right = (css_right * scale_x)
            .ceil()
            .min(image.width as f64) as u32;
        let bottom = (css_bottom * scale_y)
            .ceil()
            .min(image.height as f64) as u32;
        if right <= left || bottom <= top {
            continue;
        }

        for y in top..bottom {
            for x in left..right {
                let offset = ((y * image.width + x) * 4) as usize;
                image.data[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        applied += 1;
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pixel_diff_is_local() {
        let a = RgbaImage {
            width: 2,
            height: 2,
            data: vec![0; 16],
        };
        let mut b = a.clone();
        b.data[4] = 255;
        let d = pixel_diff(&a, &b, 1).unwrap();
        assert_eq!(d.changed_pixels, 1);
        assert_eq!(d.bounding_box.unwrap().x, 1);
    }
}