#![forbid(unsafe_code)]

use localview_protocol::Rect;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisualError {
    #[error("image dimensions do not match buffer length")]
    InvalidBuffer,
    #[error("images have different dimensions")]
    DimensionMismatch,
    #[error("CSS viewport dimensions are invalid")]
    InvalidViewport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn validate(&self) -> Result<(), VisualError> {
        if self.data.len() != self.width as usize * self.height as usize * 4 {
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

/// Redacts viewport-relative CSS rectangles directly in an RGBA image.
///
/// Rectangle coordinates are expressed in CSS pixels. The function scales them
/// to the native pixel dimensions, clamps them to the frame, and fills every
/// affected pixel with opaque black. It returns the number of rectangles that
/// intersected the captured frame.
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

    let scale_x = image.width as f64 / css_width;
    let scale_y = image.height as f64 / css_height;
    let mut applied = 0usize;

    for rect in rects {
        if !rect.x.is_finite()
            || !rect.y.is_finite()
            || !rect.width.is_finite()
            || !rect.height.is_finite()
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            continue;
        }

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
