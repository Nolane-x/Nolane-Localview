#![forbid(unsafe_code)]

mod process_metrics;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{Context, Result};
use chrono::Utc;
use localview_control::{runtime_resource_governor_for_sessions, ControlState};
use localview_core::RuntimeConfig;
use localview_discovery::{CommandListenerSource, DiscoveryEngine};
use localview_evidence::EvidenceStore;
use localview_live_bridge::LiveBridge;
use localview_observation::ObservationBus;
use localview_protocol::ObservationEvent;
use localview_security::generate_control_token;
use localview_sessions::SessionManager;
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
    let observations = ObservationBus::new(1024);
    let live = LiveBridge::default();
    let evidence = EvidenceStore::default();
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
    Ok(())
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
