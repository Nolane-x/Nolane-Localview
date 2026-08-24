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
