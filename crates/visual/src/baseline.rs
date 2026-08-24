use std::{collections::BTreeMap, sync::Arc};

use localview_protocol::SessionId;

use crate::{RgbaImage, VisualError};

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
