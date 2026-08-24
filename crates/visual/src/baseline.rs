use std::{collections::BTreeMap, sync::Arc};

use localview_protocol::{Rect, SessionId};

use crate::{RgbaImage, VisualError, MAX_DECODED_IMAGE_BYTES};

impl RgbaImage {
    /// Crops one viewport-relative CSS rectangle directly from an already decoded
    /// RGBA frame. The source image is never mutated, allowing multiple changed
    /// regions to share one bounded decode of the private-redacted viewport.
    pub fn crop_css_rect(
        &self,
        css_viewport: (f64, f64),
        rect: &Rect,
    ) -> Result<RgbaImage, VisualError> {
        self.validate()?;
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

        let scale_x = self.width as f64 / css_width;
        let scale_y = self.height as f64 / css_height;
        let left = (css_left * scale_x).floor().max(0.0) as u32;
        let top = (css_top * scale_y).floor().max(0.0) as u32;
        let right = (css_right * scale_x).ceil().min(self.width as f64) as u32;
        let bottom = (css_bottom * scale_y).ceil().min(self.height as f64) as u32;
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

        let source_row_bytes = usize::try_from(self.width)
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
            let row = self
                .data
                .get(row_start..row_end)
                .ok_or(VisualError::InvalidBuffer)?;
            data.extend_from_slice(row);
        }

        Ok(RgbaImage {
            width,
            height,
            data,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualBaselineContext {
    pub route: String,
    pub css_width: u32,
    pub css_height: u32,
    pub device_scale_factor: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug)]
struct VisualBaselineEntry {
    context: VisualBaselineContext,
    image: Arc<RgbaImage>,
    bytes: usize,
    touched_at: u64,
}

#[derive(Debug)]
pub struct VisualBaselineCache {
    entries: BTreeMap<SessionId, VisualBaselineEntry>,
    byte_budget: usize,
    max_entries: usize,
    used_bytes: usize,
    clock: u64,
}

impl VisualBaselineCache {
    pub fn new(byte_budget: usize, max_entries: usize) -> Result<Self, VisualError> {
        if byte_budget == 0 || max_entries == 0 {
            return Err(VisualError::InvalidBaselinePolicy);
        }
        Ok(Self {
            entries: BTreeMap::new(),
            byte_budget,
            max_entries,
            used_bytes: 0,
            clock: 0,
        })
    }

    pub fn get_compatible(
        &mut self,
        session_id: SessionId,
        context: &VisualBaselineContext,
    ) -> Option<Arc<RgbaImage>> {
        let compatible = self
            .entries
            .get(&session_id)
            .is_some_and(|entry| entry.context == *context);
        if !compatible {
            self.remove(session_id);
            return None;
        }

        let touched_at = self.next_tick();
        let entry = self
            .entries
            .get_mut(&session_id)
            .expect("compatible baseline was checked above");
        entry.touched_at = touched_at;
        Some(entry.image.clone())
    }

    pub fn insert(
        &mut self,
        session_id: SessionId,
        context: VisualBaselineContext,
        image: Arc<RgbaImage>,
    ) -> Result<bool, VisualError> {
        image.validate()?;
        if context.pixel_width != image.width || context.pixel_height != image.height {
            return Err(VisualError::DimensionMismatch);
        }

        self.remove(session_id);
        let bytes = image.data.len();
        if bytes > self.byte_budget {
            return Ok(false);
        }

        let touched_at = self.next_tick();
        self.entries.insert(
            session_id,
            VisualBaselineEntry {
                context,
                image,
                bytes,
                touched_at,
            },
        );
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.evict_to_budget();
        Ok(self.entries.contains_key(&session_id))
    }

    pub fn remove(&mut self, session_id: SessionId) -> bool {
        let Some(entry) = self.entries.remove(&session_id) else {
            return false;
        };
        self.used_bytes = self.used_bytes.saturating_sub(entry.bytes);
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    fn next_tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    fn evict_to_budget(&mut self) {
        while self.used_bytes > self.byte_budget || self.entries.len() > self.max_entries {
            let Some(lru_session) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.touched_at)
                .map(|(session_id, _)| *session_id)
            else {
                break;
            };
            self.remove(lru_session);
        }
    }
}