#![forbid(unsafe_code)]

use std::{net::{IpAddr, Ipv4Addr, SocketAddr}, path::PathBuf, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use anyhow::{Context, Result};
use chrono::Utc;
use localview_control::ControlState;
use localview_core::RuntimeConfig;
use localview_discovery::{CommandListenerSource, DiscoveryEngine};
use localview_observation::ObservationBus;
use localview_protocol::ObservationEvent;
use localview_security::generate_control_token;
use localview_sessions::SessionManager;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "localview=info".into())).init();
    let config=RuntimeConfig::default();
    let sessions=Arc::new(SessionManager::new(config.disconnect_grace));
    let observations=ObservationBus::new(1024);
    let paused=Arc::new(AtomicBool::new(matches!(config.auto_open,localview_core::AutoOpenMode::Paused)));
    let token=load_or_create_token().await?;
    let control_state=ControlState{token:Arc::from(token.clone()),sessions:sessions.clone(),observations:observations.clone(),paused:paused.clone()};
    let addr=SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST),config.control_port);
    tokio::spawn(async move { if let Err(e)=localview_control::serve(addr,control_state).await { tracing::error!(error=%e,"control plane stopped"); } });
    info!(%addr,"LocalView daemon ready");

    let discovery=DiscoveryEngine::new(CommandListenerSource,config.probe_timeout,config.probe_concurrency)?;
    let mut interval=tokio::time::interval(config.scan_interval);
    loop {
        tokio::select! {
            _=tokio::signal::ctrl_c()=>{ info!("shutdown requested"); break; }
            _=interval.tick()=>{
                if paused.load(Ordering::Relaxed){continue;}
                match discovery.scan().await {
                    Ok(found)=>{
                        let result=sessions.reconcile(found,Utc::now()).await;
                        for id in result.created { if let Some(s)=sessions.get(id).await { observations.publish(ObservationEvent::ServerDetected{session_id:id,endpoint:s.endpoint}).await; } }
                        for id in result.disconnected { observations.publish(ObservationEvent::ServerDisconnected{session_id:id}).await; }
                        for id in result.reconnected { observations.publish(ObservationEvent::ServerReconnected{session_id:id}).await; }
                    }
                    Err(e)=>warn!(error=%e,"discovery scan failed"),
                }
            }
        }
    }
    Ok(())
}

async fn load_or_create_token()->Result<String>{
    let dir=state_dir()?; tokio::fs::create_dir_all(&dir).await?; let path=dir.join("control.token");
    if let Ok(existing)=tokio::fs::read_to_string(&path).await { let token=existing.trim(); if !token.is_empty(){return Ok(token.to_owned());} }
    let token=generate_control_token(); tokio::fs::write(&path,&token).await.context("write control token")?; Ok(token)
}
fn state_dir()->Result<PathBuf>{ dirs::data_local_dir().map(|p|p.join("LocalView")).context("no local data directory") }
