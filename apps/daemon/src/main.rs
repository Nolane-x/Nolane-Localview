#![forbid(unsafe_code)]

mod process_metrics;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::Utc;
use localview_chromium::discover_chromium_executable;
use localview_control::{
    configure_chromium_executor_for_sessions, configure_windows_observe_runtime_for_sessions,
    runtime_resource_governor_for_sessions, ControlState,
};
use localview_core::RuntimeConfig;
use localview_discovery::{CommandListenerSource, DiscoveryEngine};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::ObservationEvent;
use localview_security::generate_control_token;
use localview_sessions::SessionManager;
use localview_windows_observe_runtime::{
    spawn_windows_uia_runtime_manager, WindowsObserveRuntimeConfig,
    WindowsUiaObserveRuntimeManager,
};
use localview_windows_uia_provider::WindowsUiaWorkerConfig;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "localview=info".into()),
        )
        .init();

    let config = RuntimeConfig::default();
    let sessions = Arc::new(SessionManager::new(config.disconnect_grace));
    let resources = runtime_resource_governor_for_sessions(&sessions);
    process_metrics::spawn(resources.clone());
    if let Some(executable) = discover_chromium_executable() {
        let temp_root = state_dir()?.join("chromium-runtime");
        configure_chromium_executor_for_sessions(&sessions, executable.clone(), temp_root);
        info!(
            executable = %executable.display(),
            "Tier-3 Chromium executor available"
        );
    } else {
        info!("Tier-3 Chromium executor unavailable; browser-specific probes fail closed");
    }
    let observations = ObservationBus::new(1024);
    let live = LiveBridge::default();
    let evidence = EvidenceStore::default();

    #[cfg(windows)]
    let windows_observe: Option<Arc<WindowsUiaObserveRuntimeManager>> =
        match spawn_windows_uia_runtime_manager(
            live.clone(),
            WindowsUiaWorkerConfig::default(),
            WindowsObserveRuntimeConfig::default(),
        ) {
            Ok(runtime) => {
                let runtime = Arc::new(runtime);
                info!("Windows UIA observe-only runtime available");
                Some(runtime)
            }
            Err(error) => {
                warn!(%error, "Windows UIA observe-only runtime unavailable; attachment routes fail closed");
                None
            }
        };

    #[cfg(not(windows))]
    let windows_observe: Option<Arc<WindowsUiaObserveRuntimeManager>> = None;

    configure_windows_observe_runtime_for_sessions(&sessions, windows_observe.clone());
    if let Some(runtime) = windows_observe.clone() {
        spawn_windows_observe_drain_loop(runtime);
    }

    let paused = Arc::new(AtomicBool::new(matches!(
        config.auto_open,
        localview_core::AutoOpenMode::Paused
    )));
    let token = load_or_create_token().await?;
    let control_state = ControlState {
        token: Arc::from(token.clone()),
        sessions: sessions.clone(),
        observations: observations.clone(),
        live: live.clone(),
        evidence: evidence.clone(),
        paused: paused.clone(),
    };
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.control_port);
    tokio::spawn(async move {
        if let Err(error) = localview_control::serve(addr, control_state).await {
            tracing::error!(%error, "control plane stopped");
        }
    });
    info!(%addr, "LocalView daemon ready");

    let discovery = DiscoveryEngine::new(
        CommandListenerSource,
        config.probe_timeout,
        config.probe_concurrency,
    )?;
    let mut interval = tokio::time::interval(config.scan_interval);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown requested");
                break;
            }
            _ = interval.tick() => {
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
                match discovery.scan().await {
                    Ok(found) => {
                        let result = sessions.reconcile(found, Utc::now()).await;
                        for id in result.created {
                            if let Some(session) = sessions.get(id).await {
                                observations.publish(ObservationEvent::ServerDetected {
                                    session_id: id,
                                    endpoint: session.endpoint,
                                }).await;
                            }
                        }
                        for id in result.disconnected {
                            observations.publish(ObservationEvent::ServerDisconnected { session_id: id }).await;
                        }
                        for id in result.reconnected {
                            observations.publish(ObservationEvent::ServerReconnected { session_id: id }).await;
                        }
                        for id in result.removed {
                            if let Some(runtime) = &windows_observe {
                                if runtime.status(id).await.is_some() {
                                    if let Err(error) = runtime.release(id).await {
                                        warn!(session_id = %id, %error, "Windows observe provider cleanup failed after local authority was detached");
                                    }
                                }
                            }
                            live.release_session(id).await;
                            evidence.release_session(id).await;
                            resources.release_session(&id.to_string());
                        }
                    }
                    Err(error) => warn!(%error, "discovery scan failed"),
                }
            }
        }
    }

    if let Some(runtime) = &windows_observe {
        for id in runtime.attached_sessions().await {
            if let Err(error) = runtime.release(id).await {
                warn!(session_id = %id, %error, "Windows observe provider cleanup failed after shutdown detach");
            }
        }
    }
    configure_windows_observe_runtime_for_sessions(&sessions, None);
    Ok(())
}

fn spawn_windows_observe_drain_loop(runtime: Arc<WindowsUiaObserveRuntimeManager>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            for session_id in runtime.attached_sessions().await {
                if let Err(error) = runtime.drain_once(session_id).await {
                    warn!(%session_id, %error, "Windows observe callback drain failed; detaching fail-closed");
                    if let Err(cleanup_error) = runtime.release(session_id).await {
                        warn!(%session_id, %cleanup_error, "Windows observe provider cleanup failed after drain-error detach");
                    }
                }
            }
        }
    });
}

async fn load_or_create_token() -> Result<String> {
    let dir = state_dir()?;
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join("control.token");
    if let Ok(existing) = tokio::fs::read_to_string(&path).await {
        let token = existing.trim();
        if !token.is_empty() {
            return Ok(token.to_owned());
        }
    }
    let token = generate_control_token();
    tokio::fs::write(&path, &token)
        .await
        .context("write control token")?;
    Ok(token)
}

fn state_dir() -> Result<PathBuf> {
    dirs::data_local_dir()
        .map(|path| path.join("LocalView"))
        .context("no local data directory")
}
