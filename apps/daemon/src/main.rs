#![forbid(unsafe_code)]

mod consequential_recovery;
mod process_metrics;

use std::{
    collections::HashMap,
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
use localview_live_bridge::{ConsequentialJournal, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::{
    ObservationEvent, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use localview_security::generate_control_token;
use localview_sessions::SessionManager;
use localview_windows_observe_runtime::{
    WindowsObserveRuntimeError, WindowsUiaObserveRuntimeManager,
};
#[cfg(windows)]
use localview_windows_observe_runtime::{
    spawn_windows_uia_runtime_manager_with_governor, WindowsObserveRuntimeConfig,
};
#[cfg(windows)]
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
    let consequential_recovery =
        consequential_recovery::open_boot_consequential_recovery(&state_dir()?).await?;
    let consequential_journal = consequential_recovery.journal().clone();
    if consequential_recovery.inventory().is_empty() {
        info!(
            journal = %consequential_recovery.journal_path().display(),
            "durable consequential recovery journal replayed with no action history"
        );
    } else {
        warn!(
            actions = consequential_recovery.inventory().len(),
            journal = %consequential_recovery.journal_path().display(),
            "durable consequential action history replayed; no process-local dispatch authority was restored"
        );
        for entry in consequential_recovery.inventory() {
            info!(
                action_id = %entry.action_id,
                recovery_state = ?entry.recovery_state,
                latest_journal_sequence = entry.latest_journal_sequence,
                "replayed durable consequential recovery inventory entry"
            );
        }
    }
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
        match spawn_windows_uia_runtime_manager_with_governor(
            live.clone(),
            resources.clone(),
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
        spawn_windows_observe_drain_loop(runtime, consequential_journal.clone());
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
    // Keep the reopened durable journal authority alive for the full daemon
    // lifetime. Attachment recovery may commit an already-verified durable
    // outcome, but restart never recreates a dispatch permit.
    drop(consequential_journal);
    drop(consequential_recovery);
    Ok(())
}

fn spawn_windows_observe_drain_loop(
    runtime: Arc<WindowsUiaObserveRuntimeManager>,
    journal: Arc<ConsequentialJournal>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut recovered_attachments: HashMap<
            SessionId,
            (ProviderIncarnationRef, TargetIncarnationRef),
        > = HashMap::new();

        loop {
            interval.tick().await;
            let attached_sessions = runtime.attached_sessions().await;
            recovered_attachments
                .retain(|session_id, _| attached_sessions.contains(session_id));

            for session_id in attached_sessions {
                if let Some(snapshot) = runtime.current_semantic_snapshot(session_id).await {
                    let lineage = (
                        snapshot.provider_incarnation_ref().clone(),
                        snapshot.target_incarnation_ref().clone(),
                    );
                    if recovered_attachments.get(&session_id) != Some(&lineage) {
                        match consequential_recovery::process_windows_attachment_recovery(
                            journal.as_ref(),
                            runtime.as_ref(),
                            session_id,
                        )
                        .await
                        {
                            Ok(report) => {
                                for action_id in report.committed_action_ids {
                                    info!(
                                        %session_id,
                                        %action_id,
                                        "committed durable VerifiedExpected consequential recovery after exact Windows attachment"
                                    );
                                }
                                for action_id in report.historical_committed_action_ids {
                                    info!(
                                        %session_id,
                                        %action_id,
                                        "validated historical committed consequential recovery after exact Windows attachment"
                                    );
                                }
                                for debt in report.verifier_required {
                                    warn!(
                                        %session_id,
                                        action_id = %debt.action_id,
                                        recovery_state = ?debt.recovery_state,
                                        latest_journal_sequence = debt.latest_journal_sequence,
                                        expected_postconditions = ?debt.expected_postcondition_contract_refs,
                                        "durable consequential recovery is exact-lineage bound but requires an independent registered verifier; no observation or dispatch authority was fabricated"
                                    );
                                }
                                recovered_attachments.insert(session_id, lineage);
                            }
                            Err(error) => {
                                warn!(
                                    %session_id,
                                    %error,
                                    "attachment-bound consequential recovery failed closed; durable debt remains retryable as recovery work only"
                                );
                            }
                        }
                    }
                } else {
                    warn!(
                        %session_id,
                        "attached Windows observe session has no current semantic snapshot; consequential recovery remains untouched"
                    );
                }

                match runtime.drain_once(session_id).await {
                    Ok(_) => {}
                    Err(WindowsObserveRuntimeError::ResourceDenied { .. }) => {
                        // Runtime pressure is transient admission state, not a
                        // provider/target failure. Keep the explicit attachment
                        // and any continuity debt so a later admitted drain can
                        // reconcile it without reattaching or polling globally.
                    }
                    Err(error) => {
                        warn!(%session_id, %error, "Windows observe callback drain failed; detaching fail-closed");
                        recovered_attachments.remove(&session_id);
                        if let Err(cleanup_error) = runtime.release(session_id).await {
                            warn!(%session_id, %cleanup_error, "Windows observe provider cleanup failed after drain-error detach");
                        }
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
