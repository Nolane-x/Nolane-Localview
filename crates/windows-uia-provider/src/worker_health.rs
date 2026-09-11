use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError},
    },
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerHealthError {
    Poisoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerReceiveError {
    Poisoned,
    Timeout,
    Disconnected,
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

    pub(crate) fn recv_timeout<T, E>(
        &self,
        receiver: &Receiver<Result<T, E>>,
        timeout: Duration,
    ) -> Result<Result<T, E>, WorkerReceiveError> {
        self.ensure_healthy()
            .map_err(|_| WorkerReceiveError::Poisoned)?;
        match receiver.recv_timeout(timeout) {
            Ok(result) => Ok(result),
            Err(RecvTimeoutError::Timeout) => {
                self.poison_after_timeout();
                Err(WorkerReceiveError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(WorkerReceiveError::Disconnected),
        }
    }

    pub(crate) fn poison_after_timeout(&self) {
        self.poisoned.store(true, Ordering::Release);
    }

    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::Acquire)
    }
}
