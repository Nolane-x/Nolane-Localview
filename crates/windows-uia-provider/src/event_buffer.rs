use std::collections::VecDeque;

use localview_native_provider::ProviderEventReliabilityProfile;
use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsUiaEventKind {
    PropertyChanged { property_id: i32 },
    StructureChanged { change_type: i32 },
    FocusChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaEventDraft {
    pub kind: WindowsUiaEventKind,
    pub element_ref: Option<ProviderElementRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaEvent {
    pub sequence: u64,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub kind: WindowsUiaEventKind,
    pub element_ref: Option<ProviderElementRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaEventDrain {
    pub events: Vec<WindowsUiaEvent>,
    pub dropped_before_drain: u64,
    pub latest_sequence: u64,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum WindowsUiaEventBufferError {
    #[error("Windows UIA event buffer capacity must be non-zero")]
    InvalidCapacity,
    #[error("event element provider incarnation does not match event buffer lineage")]
    ProviderIncarnationMismatch,
    #[error("event element target incarnation does not match event buffer lineage")]
    TargetIncarnationMismatch,
}

#[derive(Debug, Clone)]
pub struct WindowsUiaEventBuffer {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    capacity: usize,
    latest_sequence: u64,
    dropped_since_drain: u64,
    events: VecDeque<WindowsUiaEvent>,
    reliability_profile: ProviderEventReliabilityProfile,
}

impl WindowsUiaEventBuffer {
    pub fn new(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        capacity: usize,
    ) -> Result<Self, WindowsUiaEventBufferError> {
        if capacity == 0 {
            return Err(WindowsUiaEventBufferError::InvalidCapacity);
        }

        Ok(Self {
            provider_incarnation_ref,
            target_incarnation_ref,
            capacity,
            latest_sequence: 0,
            dropped_since_drain: 0,
            events: VecDeque::with_capacity(capacity),
            reliability_profile: ProviderEventReliabilityProfile::windows_uia_v1(),
        })
    }

    pub fn sequence_baseline(&self) -> u64 {
        self.latest_sequence
    }

    pub fn reliability_profile(&self) -> &ProviderEventReliabilityProfile {
        &self.reliability_profile
    }

    pub fn push(&mut self, draft: WindowsUiaEventDraft) -> Result<u64, WindowsUiaEventBufferError> {
        if let Some(element_ref) = &draft.element_ref {
            if element_ref.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaEventBufferError::ProviderIncarnationMismatch);
            }
            if element_ref.target_incarnation_ref != self.target_incarnation_ref {
                return Err(WindowsUiaEventBufferError::TargetIncarnationMismatch);
            }
        }

        let sequence = self.latest_sequence.saturating_add(1);
        self.latest_sequence = sequence;

        if self.events.len() == self.capacity {
            self.events.pop_front();
            self.dropped_since_drain = self.dropped_since_drain.saturating_add(1);
        }

        self.events.push_back(WindowsUiaEvent {
            sequence,
            provider_incarnation_ref: self.provider_incarnation_ref.clone(),
            target_incarnation_ref: self.target_incarnation_ref.clone(),
            kind: draft.kind,
            element_ref: draft.element_ref,
        });

        Ok(sequence)
    }

    pub fn drain(&mut self, limit: usize) -> WindowsUiaEventDrain {
        let take = limit.min(self.events.len());
        let events = self.events.drain(..take).collect();
        let dropped_before_drain = self.dropped_since_drain;
        self.dropped_since_drain = 0;

        WindowsUiaEventDrain {
            events,
            dropped_before_drain,
            latest_sequence: self.latest_sequence,
        }
    }
}
