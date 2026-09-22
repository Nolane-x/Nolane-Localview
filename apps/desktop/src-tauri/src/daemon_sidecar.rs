use std::{
    sync::Mutex,
    time::Duration,
};

use reqwest::{Client, redirect::Policy};
use serde::Deserialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

const CONTROL_HEALTH_URL: &str = "http://127.0.0.1:45454/health";
const HEALTH_TIMEOUT: Duration = Duration::from_millis(500);
const STARTUP_RETRIES: usize = 50;
const STARTUP_RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct ManagedDaemonSidecar {
    child: Mutex<Option<CommandChild>>,
}

#[derive(Debug, Deserialize)]
struct HealthReceipt {
    version: String,
    status: String,
}

enum HealthState {
    Missing,
    Ready,
    Incompatible(String),
}

async fn health_state(client: &Client) -> HealthState {
    let response = match client.get(CONTROL_HEALTH_URL).send().await {
        Ok(response) => response,
        Err(_) => return HealthState::Missing,
    };
    if !response.status().is_success() {
        return HealthState::Missing;
    }
    let receipt = match response.json::<HealthReceipt>().await {
        Ok(receipt) => receipt,
        Err(_) => return HealthState::Missing,
    };
    if receipt.status != "ready" {
        return HealthState::Missing;
    }
    if receipt.version != env!("CARGO_PKG_VERSION") {
        return HealthState::Incompatible(receipt.version);
    }
    HealthState::Ready
}

pub fn ensure_daemon(app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    tauri::async_runtime::block_on(ensure_daemon_async(app))
        .map_err(|error| error.into())
}

async fn ensure_daemon_async(app: AppHandle) -> Result<(), String> {
    let client = Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .redirect(Policy::none())
        .no_proxy()
        .build()
        .map_err(|error| format!("LocalView daemon health client unavailable: {error}"))?;

    match health_state(&client).await {
        HealthState::Ready => return Ok(()),
        HealthState::Incompatible(version) => {
            return Err(format!(
                "LocalView daemon version mismatch: desktop {} found daemon {version}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        HealthState::Missing => {}
    }

    let command = app
        .shell()
        .sidecar("localview-daemon")
        .map_err(|error| format!("bundled LocalView daemon unavailable: {error}"))?;
    let (mut events, child) = command
        .spawn()
        .map_err(|error| format!("could not start bundled LocalView daemon: {error}"))?;
    let pid = child.pid();

    {
        let state = app.state::<ManagedDaemonSidecar>();
        let mut guard = state
            .child
            .lock()
            .map_err(|_| "LocalView daemon state unavailable".to_string())?;
        *guard = Some(child);
    }

    let event_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            if matches!(event, CommandEvent::Terminated(_) | CommandEvent::Error(_)) {
                let state = event_app.state::<ManagedDaemonSidecar>();
                if let Ok(mut guard) = state.child.lock()
                    && guard.as_ref().is_some_and(|child| child.pid() == pid)
                {
                    guard.take();
                }
                break;
            }
        }
    });

    for _ in 0..STARTUP_RETRIES {
        match health_state(&client).await {
            HealthState::Ready => return Ok(()),
            HealthState::Incompatible(version) => {
                stop_owned_daemon(&app);
                return Err(format!(
                    "bundled LocalView daemon started with incompatible version {version}"
                ));
            }
            HealthState::Missing => {
                tokio::time::sleep(STARTUP_RETRY_DELAY).await;
            }
        }
    }

    stop_owned_daemon(&app);
    Err("bundled LocalView daemon did not become ready within 5 seconds".into())
}

pub fn stop_owned_daemon(app: &AppHandle) {
    let state = app.state::<ManagedDaemonSidecar>();
    let child = state
        .child
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());
    if let Some(child) = child {
        let _ = child.kill();
    }
}
