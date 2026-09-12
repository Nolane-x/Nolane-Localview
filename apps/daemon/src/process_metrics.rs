#![forbid(unsafe_code)]

use std::{thread::available_parallelism, time::Duration};

use localview_resource_governor::{normalize_process_metrics, RuntimeResourceGovernor};
use sysinfo::{get_current_pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::time::MissedTickBehavior;
use tracing::warn;

const PROCESS_RESOURCE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

pub(crate) fn spawn(governor: RuntimeResourceGovernor) {
    tokio::spawn(async move {
        if let Err(error) = sample_forever(governor).await {
            warn!(%error, "daemon process resource sampler stopped");
        }
    });
}

async fn sample_forever(governor: RuntimeResourceGovernor) -> Result<(), &'static str> {
    if !sysinfo::IS_SUPPORTED_SYSTEM {
        return Err("system telemetry is unsupported on this platform");
    }
    let pid = get_current_pid().map_err(|_| "current process id unavailable")?;
    let logical_cpus = available_parallelism().map(usize::from).unwrap_or(1);
    let refresh_kind = ProcessRefreshKind::nothing().with_memory().with_cpu();
    let mut system = System::new();
    let mut interval = tokio::time::interval(PROCESS_RESOURCE_SAMPLE_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        interval.tick().await;
        let pids = [pid];
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            refresh_kind,
        );
        let Some(process) = system.process(pid) else {
            continue;
        };
        let Some(metrics) = normalize_process_metrics(
            process.memory(),
            process.cpu_usage(),
            logical_cpus,
        ) else {
            continue;
        };
        governor.update_process_metrics(metrics.memory_mb, metrics.cpu_percent);
    }
}
