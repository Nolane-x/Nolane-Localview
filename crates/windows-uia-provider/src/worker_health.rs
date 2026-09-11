use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerHealthError {
    Poisoned,
}

#[derive(Debug, Default)]
pub(crate) struct WorkerHealth {
    poisoned: AtomicBool,
}

impl WorkerHealth {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn ensure_healthy(&self) -> Result<(), WorkerHealthError> {
        if self.poisoned.load(Ordering::Acquire) {
            Err(WorkerHealthError::Poisoned)
        } else {
            Ok(())
        }
    }

    pub(crate) fn poison_after_timeout(&self) {
        self.poisoned.store(true, Ordering::Release);
    }

    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Acquire)
    }
}
