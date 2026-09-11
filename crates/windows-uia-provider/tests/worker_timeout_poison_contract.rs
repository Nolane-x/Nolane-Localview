#[path = "../src/worker_health.rs"]
mod worker_health;

use std::{sync::mpsc, time::Duration};
use worker_health::{WorkerHealth, WorkerHealthError, WorkerReceiveError};

#[test]
fn first_timeout_poisons_worker_and_later_commands_fail_fast() {
    let health = WorkerHealth::new();
    let (_sender, receiver) = mpsc::channel::<Result<(), ()>>();

    assert_eq!(
        health.recv_timeout(&receiver, Duration::from_millis(1)),
        Err(WorkerReceiveError::Timeout)
    );
    assert_eq!(health.ensure_healthy(), Err(WorkerHealthError::Poisoned));
}

#[test]
fn fresh_worker_health_starts_unpoisoned() {
    let first = WorkerHealth::new();
    first.poison_after_timeout();

    let fresh = WorkerHealth::new();

    assert_eq!(first.ensure_healthy(), Err(WorkerHealthError::Poisoned));
    assert!(fresh.ensure_healthy().is_ok());
}
