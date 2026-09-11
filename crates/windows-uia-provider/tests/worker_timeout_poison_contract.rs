#[path = "../src/worker_health.rs"]
mod worker_health;

use worker_health::WorkerHealth;

#[test]
fn first_timeout_poisons_worker_and_later_commands_fail_fast() {
    let health = WorkerHealth::new();

    assert!(health.ensure_healthy().is_ok());

    health.poison_after_timeout();

    assert!(health.ensure_healthy().is_err());
    assert!(health.is_poisoned());
}

#[test]
fn fresh_worker_health_starts_unpoisoned() {
    let first = WorkerHealth::new();
    first.poison_after_timeout();

    let fresh = WorkerHealth::new();

    assert!(first.is_poisoned());
    assert!(!fresh.is_poisoned());
    assert!(fresh.ensure_healthy().is_ok());
}
