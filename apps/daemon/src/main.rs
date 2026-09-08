#![forbid(unsafe_code)]

mod consequential_recovery;
mod process_metrics;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
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
    configure_chromium_executor_for_sessions, configure_surface_recovery_journal_for_sessions,
    configure_windows_consequential_control_for_sessions,
    configure_windows_observe_runtime_for_sessions,
    release_windows_consequential_control_session_for_sessions, runtime_resource_governor_for_sessions,
    ControlState, SurfaceRecoveryJournal, SURFACE_RECOVERY_JOURNAL_FILE,
};
use localview_core::RuntimeConfig;
use localview_discovery::{CommandListenerSource, DiscoveryEngine};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{ConsequentialJournal, ConsequentialRecoveryActionScope, LiveBridge};
use localview_observation::ObservationBus;
use localview_protocol::ObservationEvent;
use localview_security::generate_control_token;
use localview_sessions::{
    SessionIdentityHealth, SessionIdentityResolver, SessionManager, SESSION_IDENTITY_REGISTRY_FILE,
};
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
    let state_root = state_dir()?;
    tokio::fs::create_dir_all(&state_root)
        .await
        .context("create LocalView state directory")?;

    let identity_resolver =
        SessionIdentityResolver::open_file(state_root.join(SESSION_IDENTITY_REGISTRY_FILE)).await;
    if identity_resolver.health() == SessionIdentityHealth::VolatileDegraded {
        warn!(
            diagnostic = ?identity_resolver.diagnostic(),
            "session identity continuity degraded; using volatile session identity"
        );
    }
    let sessions = Arc::new(SessionManager::with_identity_resolver(
        config.disconnect_grace,
        identity_resolver,
    ));

    let surface_recovery_journal = Arc::new(
        SurfaceRecoveryJournal::open(state_root.join(SURFACE_RECOVERY_JOURNAL_FILE))
            .await
            .context("open durable native surface recovery journal")?,
    );
    if surface_recovery_journal.outstanding_len() == 0 {
        info!(
            journal = %surface_recovery_journal.path().display(),
            "durable native surface recovery journal replayed with no outstanding debt"
        );
    } else {
        warn!(
            outstanding = surface_recovery_journal.outstanding_len(),
            journal = %surface_recovery_journal.path().display(),
            "durable native surface recovery debt replayed; no live lease, visibility, provider, action, or evidence authority was restored"
        );
    }
    configure_surface_recovery_journal_for_sessions(
        &sessions,
        Some(surface_recovery_journal.clone()),
    );

    let consequential_recovery =
        consequential_recovery::open_boot_consequential_recovery(&state_root).await?;
    let consequential_journal = consequential_recovery.journal().clone();
    let consequential_boot_scope = consequential_recovery.scope().clone();
    let has_consequential_boot_history = !consequential_boot_scope.is_empty();
    if !has_consequential_boot_history {
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
    let resources = runtime_resource_governor_for_sessions(&sessions);
    process_metrics::spawn(resources.clone());
    if let Some(executable) = discover_chromium_executable() {
        let temp_root = state_root.join("chromium-runtime");
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
    configure_windows_consequential_control_for_sessions(
        &sessions,
        Some(consequential_journal.clone()),
    );
    if let Some(runtime) = windows_observe.clone() {
        spawn_windows_observe_drain_loop(runtime.clone(), sessions.clone());
        if has_consequential_boot_history {
            spawn_windows_consequential_recovery_loop(
                runtime,
                live.clone(),
                consequential_journal.clone(),
                consequential_boot_scope,
            );
        }
    }

    let paused = Arc::new(AtomicBool::new(matches!(
        config.auto_open,
        localview_core::AutoOpenMode::Paused
    )));
    let token = load_or_create_token(&state_root).await?;
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
                            release_windows_consequential_control_session_for_sessions(&sessions, id).await;
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
            release_windows_consequential_control_session_for_sessions(&sessions, id).await;
        }
    }
    configure_windows_observe_runtime_for_sessions(&sessions, None);
    configure_windows_consequential_control_for_sessions(&sessions, None);
    configure_surface_recovery_journal_for_sessions(&sessions, None);
    // Durable recovery journals remain alive for the full daemon lifetime.
    // Replay may restore only recovery debt/history; restart never recreates a
    // live surface lease, visibility truth, dispatch permit, confirmation
    // capability, provider, action, executor, or evidence authority.
    drop(surface_recovery_journal);
    drop(consequential_journal);
    drop(consequential_recovery);
    Ok(())
}

fn spawn_windows_observe_drain_loop(
    runtime: Arc<WindowsUiaObserveRuntimeManager>,
    sessions: Arc<SessionManager>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            for session_id in runtime.attached_sessions().await {
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
                        if let Err(cleanup_error) = runtime.release(session_id).await {
                            warn!(%session_id, %cleanup_error, "Windows observe provider cleanup failed after drain-error detach");
                        }
                        release_windows_consequential_control_session_for_sessions(
                            &sessions,
                            session_id,
                        )
                        .await;
                    }
                }
            }
        }
    });
}

fn spawn_windows_consequential_recovery_loop(
    runtime: Arc<WindowsUiaObserveRuntimeManager>,
    live: LiveBridge,
    journal: Arc<ConsequentialJournal>,
    scope: ConsequentialRecoveryActionScope,
) {
    tokio::spawn(async move {
        let verifier = consequential_recovery::FailClosedWindowsPostconditionVerifier;
        let mut tracker = consequential_recovery::WindowsBootRecoveryTracker::default();
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            let attempts = consequential_recovery::recover_newly_attached_boot_debt(
                &live,
                journal.as_ref(),
                runtime.as_ref(),
                &verifier,
                &scope,
                &mut tracker,
            )
            .await;

            for attempt in attempts {
                match attempt.outcome {
                    Ok(drain) => {
                        if drain.entries.is_empty() {
                            info!(
                                session_id = %drain.session_id,
                                provider_incarnation_ref = ?drain.provider_incarnation_ref,
                                target_incarnation_ref = ?drain.target_incarnation_ref,
                                "exact Windows attachment had no matching consequential boot recovery debt"
                            );
                            continue;
                        }
                        for outcome in drain.entries {
                            info!(
                                session_id = %drain.session_id,
                                provider_incarnation_ref = ?drain.provider_incarnation_ref,
                                target_incarnation_ref = ?drain.target_incarnation_ref,
                                recovery_outcome = ?outcome,
                                "processed attachment-bound consequential boot recovery debt"
                            );
                        }
                    }
                    Err(error) => {
                        warn!(
                            session_id = %attempt.session_id,
                            provider_incarnation_ref = ?attempt.provider_incarnation_ref,
                            target_incarnation_ref = ?attempt.target_incarnation_ref,
                            %error,
                            "attachment-bound consequential boot recovery failed fail-closed; this exact lineage remains retryable while later attachments continue"
                        );
                    }
                }
            }
        }
    });
}

async fn load_or_create_token(state_root: &Path) -> Result<String> {
    tokio::fs::create_dir_all(state_root).await?;
    let path = state_root.join("control.token");
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
