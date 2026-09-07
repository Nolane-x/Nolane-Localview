use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RetainedResourceKind {
    CaptureStorage,
    Cache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedResourceBudget {
    pub capture_storage_bytes: u64,
    pub cache_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetainedResourceUsage {
    pub capture_storage_bytes: u64,
    pub cache_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedResourceViolation {
    pub kind: RetainedResourceKind,
    pub current_bytes: u64,
    pub projected_or_observed_bytes: u64,
    pub limit_bytes: u64,
}

#[derive(Debug)]
struct RetainedResourceState {
    budget: RetainedResourceBudget,
    usage: RetainedResourceUsage,
}

#[derive(Clone, Debug)]
pub struct RetainedResourceLedger {
    inner: Arc<Mutex<RetainedResourceState>>,
}

impl RetainedResourceLedger {
    pub fn new(budget: RetainedResourceBudget) -> Result<Self, RetainedResourceViolation> {
        if budget.capture_storage_bytes == 0 {
            return Err(RetainedResourceViolation {
                kind: RetainedResourceKind::CaptureStorage,
                current_bytes: 0,
                projected_or_observed_bytes: 0,
                limit_bytes: 0,
            });
        }
        if budget.cache_bytes == 0 {
            return Err(RetainedResourceViolation {
                kind: RetainedResourceKind::Cache,
                current_bytes: 0,
                projected_or_observed_bytes: 0,
                limit_bytes: 0,
            });
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(RetainedResourceState {
                budget,
                usage: RetainedResourceUsage::default(),
            })),
        })
    }

    pub fn usage(&self) -> RetainedResourceUsage {
        lock(&self.inner).usage
    }

    pub fn admit_projected(
        &self,
        kind: RetainedResourceKind,
        projected_bytes: u64,
    ) -> Result<(), RetainedResourceViolation> {
        let state = lock(&self.inner);
        let (current_bytes, limit_bytes) = selected(&state, kind);
        if current_bytes > limit_bytes || projected_bytes > limit_bytes {
            return Err(RetainedResourceViolation {
                kind,
                current_bytes,
                projected_or_observed_bytes: projected_bytes,
                limit_bytes,
            });
        }
        Ok(())
    }

    pub fn synchronize(
        &self,
        kind: RetainedResourceKind,
        actual_bytes: u64,
    ) -> Result<(), RetainedResourceViolation> {
        let mut state = lock(&self.inner);
        let current_bytes = match kind {
            RetainedResourceKind::CaptureStorage => {
                let previous = state.usage.capture_storage_bytes;
                state.usage.capture_storage_bytes = actual_bytes;
                previous
            }
            RetainedResourceKind::Cache => {
                let previous = state.usage.cache_bytes;
                state.usage.cache_bytes = actual_bytes;
                previous
            }
        };
        let limit_bytes = limit(&state, kind);
        if actual_bytes > limit_bytes {
            return Err(RetainedResourceViolation {
                kind,
                current_bytes,
                projected_or_observed_bytes: actual_bytes,
                limit_bytes,
            });
        }
        Ok(())
    }
}

fn selected(state: &RetainedResourceState, kind: RetainedResourceKind) -> (u64, u64) {
    match kind {
        RetainedResourceKind::CaptureStorage => (
            state.usage.capture_storage_bytes,
            state.budget.capture_storage_bytes,
        ),
        RetainedResourceKind::Cache => (state.usage.cache_bytes, state.budget.cache_bytes),
    }
}

fn limit(state: &RetainedResourceState, kind: RetainedResourceKind) -> u64 {
    match kind {
        RetainedResourceKind::CaptureStorage => state.budget.capture_storage_bytes,
        RetainedResourceKind::Cache => state.budget.cache_bytes,
    }
}

fn lock<T>(inner: &Mutex<T>) -> MutexGuard<'_, T> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
