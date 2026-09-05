use std::time::Duration;

use localview_native_provider::SnapshotBudget;
#[cfg(not(windows))]
use localview_windows_uia_provider::WindowsUiaWorkerError;
use localview_windows_uia_provider::{
    WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
};

#[test]
fn worker_config_is_bounded_and_observe_only() {
    let config = WindowsUiaWorkerConfig {
        snapshot_budget: SnapshotBudget {
            max_nodes: 64,
            max_depth: 8,
            max_properties: 512,
        },
        command_timeout: Duration::from_secs(2),
    };

    assert_eq!(config.snapshot_budget.max_nodes, 64);
    assert_eq!(config.command_timeout, Duration::from_secs(2));
    let capabilities = WindowsUiaWorker::capabilities();
    assert!(capabilities.semantic_snapshot);
    assert!(capabilities.event_subscription);
    assert!(capabilities.reconciliation);
    assert!(capabilities.resource_accounting);
    assert!(!capabilities.write_actions);
    assert!(!capabilities.input_dispatch);
}

#[test]
fn snapshot_request_requires_an_explicit_cut_and_surface_scope() {
    let request = WindowsUiaSnapshotRequest {
        snapshot_cut_ref: "cut:uia:1".into(),
        surface_scope: "window:1234".into(),
    };

    assert_eq!(request.snapshot_cut_ref, "cut:uia:1");
    assert_eq!(request.surface_scope, "window:1234");
}

#[cfg(not(windows))]
#[test]
fn spawning_windows_worker_is_explicitly_unsupported_off_windows() {
    let error = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig::default()).unwrap_err();
    assert_eq!(error, WindowsUiaWorkerError::UnsupportedPlatform);
}
