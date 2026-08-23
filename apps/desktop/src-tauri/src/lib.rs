#![forbid(unsafe_code)]

use std::path::PathBuf;

use localview_instrumentation::{bootstrap_script, InstrumentationConfig};
use localview_live_bridge::{
    BridgeAction, BridgeActionResult, IngestReport, ObserverBatch,
};
use localview_protocol::{Health, Session, SessionId};
use serde::Serialize;
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Serialize)]
struct DashboardState {
    health: Health,
    sessions: Vec<Session>,
    engine: EngineInfo,
    capabilities: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct EngineInfo {
    native: &'static str,
    tier3: &'static str,
}

#[tauri::command]
async fn dashboard_state() -> Result<DashboardState, String> {
    let client = control_client()?;
    let health = client
        .get("http://127.0.0.1:45454/health")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Health>()
        .await
        .map_err(err)?;
    let token = read_token().await?;
    let sessions = client
        .get("http://127.0.0.1:45454/v1/sessions")
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<Session>>()
        .await
        .map_err(err)?;
    Ok(DashboardState {
        health,
        sessions,
        engine: EngineInfo {
            native: native_engine(),
            tier3: "Chromium / Playwright on demand",
        },
        capabilities: vec![
            "Discovery",
            "Sessions",
            "Observation",
            "Instrumentation",
            "Live Bridge",
            "Semantic Diff",
            "Layout",
            "Visual Diff",
            "Responsive",
            "Source Map",
            "Source Graph",
            "Network",
            "Console",
            "A11y",
            "Performance",
            "Capture",
            "Flow Replay",
            "Design Grammar",
            "Diagnostics",
            "Reports",
            "Token Budget",
            "Evidence",
            "Causal Runtime",
            "Contracts",
            "State Space",
            "Counterfactual",
            "Verification",
            "MCP",
        ],
    })
}

#[tauri::command]
async fn pause_runtime() -> Result<(), String> {
    post_control("/v1/runtime/pause").await
}

#[tauri::command]
async fn resume_runtime() -> Result<(), String> {
    post_control("/v1/runtime/resume").await
}

#[tauri::command]
async fn open_preview(
    app: tauri::AppHandle,
    session_id: String,
    url: String,
    title: String,
) -> Result<(), String> {
    let session = session_id.parse::<SessionId>().map_err(err)?;
    let label = preview_label(session);
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(err)?;
        window.set_focus().map_err(err)?;
        return Ok(());
    }

    let parsed = url::Url::parse(&url).map_err(err)?;
    if !preview_navigation_allowed(&parsed) {
        return Err("LocalView preview refuses non-loopback top-level navigation".into());
    }

    let initialization_script = format!(
        "{}\n{}",
        bootstrap_script(&InstrumentationConfig::default()),
        preview_bridge_script(session)
    );

    WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(format!("{title} — LocalView"))
        .inner_size(1280.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .initialization_script(initialization_script)
        .on_navigation(preview_navigation_allowed)
        .build()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn preview_ingest(
    webview_window: tauri::WebviewWindow,
    batch: ObserverBatch,
) -> Result<IngestReport, String> {
    ensure_preview_caller(&webview_window, batch.session_id)?;
    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{}/observer",
            batch.session_id
        ))
        .bearer_auth(token)
        .json(&batch)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<IngestReport>()
        .await
        .map_err(err)
}

#[tauri::command]
async fn preview_take_actions(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
) -> Result<Vec<BridgeAction>, String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    control_client()?
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<BridgeAction>>()
        .await
        .map_err(err)
}

#[tauri::command]
async fn preview_complete_action(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
    result: BridgeActionResult,
) -> Result<(), String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/results"
        ))
        .bearer_auth(token)
        .json(&result)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

fn ensure_preview_caller(
    webview_window: &tauri::WebviewWindow,
    session_id: SessionId,
) -> Result<(), String> {
    let expected = preview_label(session_id);
    if webview_window.label() != expected {
        return Err("preview bridge session/window mismatch".into());
    }
    Ok(())
}

fn preview_label(session_id: SessionId) -> String {
    format!(
        "preview-{}",
        session_id
            .to_string()
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .take(18)
            .collect::<String>()
    )
}

fn preview_navigation_allowed(url: &url::Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1")
    )
}

async fn post_control(path: &str) -> Result<(), String> {
    let token = read_token().await?;
    control_client()?
        .post(format!("http://127.0.0.1:45454{path}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

fn control_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(err)
}

async fn read_token() -> Result<String, String> {
    tokio::fs::read_to_string(state_dir()?.join("control.token"))
        .await
        .map(|value| value.trim().to_owned())
        .map_err(err)
}

fn state_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|path| path.join("LocalView"))
        .ok_or_else(|| "no local data directory".into())
}

fn err<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn native_engine() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "WebView2 via Tauri/WRY"
    }
    #[cfg(target_os = "macos")]
    {
        "WKWebView via Tauri/WRY"
    }
    #[cfg(target_os = "linux")]
    {
        "WebKitGTK via Tauri/WRY"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "Tauri/WRY"
    }
}

fn preview_bridge_script(session_id: SessionId) -> String {
    let session = serde_json::to_string(&session_id.to_string())
        .expect("session UUID serializes to JSON string");
    PREVIEW_BRIDGE_SCRIPT.replace("__LOCALVIEW_SESSION_ID__", &session)
}

const PREVIEW_BRIDGE_SCRIPT: &str = r#"
(() => {
  if (window.__LOCALVIEW_NATIVE_BRIDGE__) return;
  const sessionId = __LOCALVIEW_SESSION_ID__;
  const generation = Date.now();
  let running = true;
  let busy = false;

  const eventKind = (type) => ({
    dom_changed: 'dom_mutation',
    route_changed: 'route',
    focus_changed: 'focus',
    scroll_changed: 'scroll',
    console: 'console',
    exception: 'runtime_error',
    unhandled_rejection: 'runtime_error',
    long_task: 'performance',
    layout_shift: 'performance',
  })[type] || null;

  const eventTime = (raw) => {
    const offset = Number(raw.at);
    const millis = Number.isFinite(offset) ? performance.timeOrigin + offset : Date.now();
    return new Date(millis).toISOString();
  };

  const normalizeEvents = (events) => events.flatMap((raw) => {
    const kind = eventKind(raw.type);
    if (!kind) return [];
    return [{
      seq: Number(raw.seq) || 0,
      captured_at: eventTime(raw),
      kind,
      reference: raw.ref || raw.refs?.[0] || null,
      route: raw.route || null,
      payload: raw,
    }];
  });

  const resolveRef = (reference) => {
    if (!reference) return null;
    const api = window.__LOCALVIEW__;
    if (!api?.refFor) return null;
    const active = document.activeElement;
    if (active && api.refFor(active) === reference) return active;
    const preferred = document.querySelectorAll('a[href],button,input,select,textarea,summary,[role],[tabindex]');
    for (const element of preferred) {
      if (api.refFor(element) === reference) return element;
    }
    for (const element of document.querySelectorAll('*')) {
      if (api.refFor(element) === reference) return element;
    }
    return null;
  };

  const setElementValue = (element, text, clearFirst) => {
    const next = clearFirst ? text : `${element.value ?? element.textContent ?? ''}${text}`;
    if (element instanceof HTMLInputElement) {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(element, next);
    } else if (element instanceof HTMLTextAreaElement) {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      setter?.call(element, next);
    } else if (element.isContentEditable) {
      element.textContent = next;
    } else {
      throw new Error('target does not accept text input');
    }
    element.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    element.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
    return next;
  };

  const keyboardOptions = (action) => {
    const modifiers = new Set((action.modifiers || []).map((value) => String(value).toLowerCase()));
    return {
      key: action.key,
      bubbles: true,
      composed: true,
      cancelable: true,
      altKey: modifiers.has('alt'),
      ctrlKey: modifiers.has('ctrl') || modifiers.has('control'),
      metaKey: modifiers.has('meta') || modifiers.has('cmd') || modifiers.has('command'),
      shiftKey: modifiers.has('shift'),
    };
  };

  const execute = async (queued) => {
    const action = queued.action || {};
    const target = queued.reference ? resolveRef(queued.reference) : null;
    switch (action.type) {
      case 'click':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.click();
        return { reference: queued.reference };
      case 'type_text':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.focus?.();
        return { reference: queued.reference, value: setElementValue(target, String(action.text ?? ''), !!action.clear_first) };
      case 'key': {
        const receiver = target || document.activeElement || document.body;
        const options = keyboardOptions(action);
        receiver.dispatchEvent(new KeyboardEvent('keydown', options));
        receiver.dispatchEvent(new KeyboardEvent('keyup', options));
        return { reference: queued.reference || null, key: action.key };
      }
      case 'scroll':
        window.scrollBy({ left: Number(action.x) || 0, top: Number(action.y) || 0, behavior: 'auto' });
        return { x: scrollX, y: scrollY };
      case 'focus':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.focus?.({ preventScroll: true });
        return { reference: queued.reference };
      case 'snapshot':
        return window.__LOCALVIEW__?.snapshot?.() ?? null;
      default:
        throw new Error(`unsupported LocalView action: ${action.type}`);
    }
  };

  const complete = async (invoke, action, ok, payload, error) => {
    await invoke('preview_complete_action', {
      sessionId,
      result: {
        action_id: action.id,
        ok,
        error: error || null,
        payload: payload ?? null,
        completed_at: new Date().toISOString(),
      },
    });
  };

  const tick = async () => {
    if (!running || busy) return;
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) {
      setTimeout(tick, 250);
      return;
    }
    busy = true;
    try {
      const api = window.__LOCALVIEW__;
      const normalized = normalizeEvents(api?.drain?.(256) || []);
      if (normalized.length) {
        await invoke('preview_ingest', {
          batch: { session_id: sessionId, generation, events: normalized },
        });
      }

      const actions = await invoke('preview_take_actions', { sessionId });
      for (const action of Array.isArray(actions) ? actions : []) {
        try {
          const payload = await execute(action);
          await complete(invoke, action, true, payload, null);
        } catch (error) {
          await complete(invoke, action, false, null, String(error?.message || error));
        }
      }
    } catch (_) {
      // The native bridge is deliberately best-effort: page rendering must never depend on it.
    } finally {
      busy = false;
      if (running) setTimeout(tick, 140);
    }
  };

  window.__LOCALVIEW_NATIVE_BRIDGE__ = Object.freeze({
    sessionId,
    generation,
    stop() { running = false; },
  });
  setTimeout(tick, 80);
})();
"#;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let menu = MenuBuilder::new(app)
                .text("show", "Open LocalView")
                .separator()
                .text("quit", "Quit LocalView")
                .build()?;
            TrayIconBuilder::new()
                .tooltip("LocalView — AI-native localhost runtime")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                    if event.id() == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_state,
            pause_runtime,
            resume_runtime,
            open_preview,
            preview_ingest,
            preview_take_actions,
            preview_complete_action
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalView desktop");
}
