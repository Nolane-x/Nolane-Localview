use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{visual_capture, workspace_surface};

const MAX_POINT_SELECT_SESSIONS: usize = 64;
const MAX_POINT_SELECT_TOKEN_BYTES: usize = 128;
const MAX_POINT_SELECT_REFERENCE_BYTES: usize = 64;
const POINT_SELECT_LEASE: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanPointSelectPhase {
    Pending,
    Selected,
    Cancelled,
    Failed,
    Stale,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanPointSelectStatus {
    pub session_id: SessionId,
    pub request_token: String,
    pub route: String,
    pub state: HumanPointSelectPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewPointSelectCompletion {
    pub request_token: String,
    pub route: String,
    pub status: String,
    pub reference: Option<String>,
    pub reason: Option<String>,
    pub bridge_generation: u64,
}

#[derive(Debug, Clone)]
struct PointSelectEntry {
    request_token: String,
    route: String,
    state: HumanPointSelectPhase,
    reference: Option<String>,
    bridge_generation: Option<u64>,
    reason: Option<&'static str>,
    started_at: Instant,
}

impl PointSelectEntry {
    fn status(&self, session_id: SessionId) -> HumanPointSelectStatus {
        HumanPointSelectStatus {
            session_id,
            request_token: self.request_token.clone(),
            route: self.route.clone(),
            state: self.state,
            reference: self.reference.clone(),
            bridge_generation: self.bridge_generation,
            reason: self.reason,
        }
    }

    fn fail(&mut self, reason: &'static str) {
        self.state = HumanPointSelectPhase::Failed;
        self.reference = None;
        self.reason = Some(reason);
    }
}

#[derive(Default)]
pub struct PointSelectState {
    entries: Mutex<HashMap<SessionId, PointSelectEntry>>,
}

impl PointSelectState {
    async fn begin(
        &self,
        session_id: SessionId,
        request_token: String,
        route: String,
    ) -> Result<HumanPointSelectStatus, String> {
        let mut entries = self.entries.lock().await;
        if !entries.contains_key(&session_id) && entries.len() >= MAX_POINT_SELECT_SESSIONS {
            entries.retain(|_, entry| entry.state == HumanPointSelectPhase::Pending);
            if entries.len() >= MAX_POINT_SELECT_SESSIONS {
                return Err("point_select_session_capacity_exceeded".into());
            }
        }

        let entry = PointSelectEntry {
            request_token,
            route,
            state: HumanPointSelectPhase::Pending,
            reference: None,
            bridge_generation: None,
            reason: None,
            started_at: Instant::now(),
        };
        let status = entry.status(session_id);
        entries.insert(session_id, entry);
        Ok(status)
    }

    async fn cancel(
        &self,
        session_id: SessionId,
        request_token: &str,
        reason: &'static str,
    ) -> HumanPointSelectStatus {
        let mut entries = self.entries.lock().await;
        let Some(entry) = entries.get_mut(&session_id) else {
            return stale_status(session_id, request_token);
        };
        if entry.request_token != request_token {
            return stale_status(session_id, request_token);
        }
        if entry.state == HumanPointSelectPhase::Pending {
            entry.state = HumanPointSelectPhase::Cancelled;
            entry.reference = None;
            entry.reason = Some(reason);
        }
        entry.status(session_id)
    }

    async fn status(
        &self,
        session_id: SessionId,
        request_token: &str,
        current_route: Result<String, String>,
    ) -> HumanPointSelectStatus {
        let mut entries = self.entries.lock().await;
        let Some(entry) = entries.get_mut(&session_id) else {
            return stale_status(session_id, request_token);
        };
        if entry.request_token != request_token {
            return stale_status(session_id, request_token);
        }
        if entry.state == HumanPointSelectPhase::Pending {
            match current_route {
                Ok(route) if route == entry.route => {
                    if entry.started_at.elapsed() > POINT_SELECT_LEASE {
                        entry.fail("expired");
                    }
                }
                Ok(_) => entry.fail("route_changed"),
                Err(_) => entry.fail("managed_surface_unavailable"),
            }
        }
        entry.status(session_id)
    }

    async fn complete(
        &self,
        session_id: SessionId,
        caller_route: &str,
        completion: PreviewPointSelectCompletion,
    ) -> Result<(), String> {
        validate_point_select_token(&completion.request_token)?;
        let completion_route = canonical_point_select_route(&completion.route)?;
        if completion_route != caller_route {
            return Err("point_select_completion_route_mismatch".into());
        }

        let mut entries = self.entries.lock().await;
        let Some(entry) = entries.get_mut(&session_id) else {
            return Ok(());
        };
        if entry.request_token != completion.request_token {
            // A newer one-shot request owns selection authority. Old completions are ignored.
            return Ok(());
        }
        if entry.state != HumanPointSelectPhase::Pending {
            return Ok(());
        }
        if entry.route != completion_route {
            entry.fail("route_changed");
            return Ok(());
        }
        if completion.bridge_generation == 0 {
            entry.fail("invalid_generation");
            return Ok(());
        }

        match completion.status.as_str() {
            "selected" => {
                let reference = completion
                    .reference
                    .as_deref()
                    .ok_or_else(|| "point_select_completion_missing_reference".to_string())?;
                if !valid_element_reference(reference) {
                    entry.fail("invalid_reference");
                    return Ok(());
                }
                entry.state = HumanPointSelectPhase::Selected;
                entry.reference = Some(reference.to_owned());
                entry.bridge_generation = Some(completion.bridge_generation);
                entry.reason = None;
            }
            "cancelled" => {
                entry.state = HumanPointSelectPhase::Cancelled;
                entry.reference = None;
                entry.bridge_generation = Some(completion.bridge_generation);
                entry.reason = Some(validate_completion_reason(completion.reason.as_deref(), "cancelled"));
            }
            "failed" => {
                entry.state = HumanPointSelectPhase::Failed;
                entry.reference = None;
                entry.bridge_generation = Some(completion.bridge_generation);
                entry.reason = Some(validate_completion_reason(completion.reason.as_deref(), "target_unavailable"));
            }
            _ => entry.fail("invalid_completion"),
        }
        Ok(())
    }
}

fn stale_status(session_id: SessionId, request_token: &str) -> HumanPointSelectStatus {
    HumanPointSelectStatus {
        session_id,
        request_token: request_token.to_owned(),
        route: String::new(),
        state: HumanPointSelectPhase::Stale,
        reference: None,
        bridge_generation: None,
        reason: Some("stale_request"),
    }
}

fn validate_completion_reason(reason: Option<&str>, fallback: &'static str) -> &'static str {
    match reason {
        Some("escape") => "escape",
        Some("cancelled") => "cancelled",
        Some("route_changed") => "route_changed",
        Some("target_changed") => "target_changed",
        Some("target_unavailable") => "target_unavailable",
        Some("invalid_reference") => "invalid_reference",
        Some("runtime_unavailable") => "runtime_unavailable",
        _ => fallback,
    }
}

fn validate_point_select_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > MAX_POINT_SELECT_TOKEN_BYTES
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("point_select_token_invalid".into());
    }
    Ok(())
}

fn valid_element_reference(reference: &str) -> bool {
    reference.len() <= MAX_POINT_SELECT_REFERENCE_BYTES
        && reference
            .strip_prefix("@e")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn canonical_point_select_route(route: &str) -> Result<String, String> {
    visual_capture::canonical_visual_diff_route(route)
        .map_err(|_| "point_select_route_invalid".to_string())
}

fn managed_point_select_eval(
    app: &tauri::AppHandle,
    session_id: SessionId,
    script: &str,
) -> Result<(), String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !workspace_surface::bridge_surface_label_allowed(window.label(), session_id) {
            return Err("point_select_surface_owner_mismatch".into());
        }
        return window.eval(script).map_err(|_| "point_select_eval_failed".to_string());
    }

    #[cfg(feature = "native-workspace")]
    {
        let workspace_label = workspace_surface::workspace_label(session_id);
        if let Some(webview) = app.get_webview(&workspace_label) {
            if !workspace_surface::bridge_surface_label_allowed(webview.label(), session_id) {
                return Err("point_select_surface_owner_mismatch".into());
            }
            return webview.eval(script).map_err(|_| "point_select_eval_failed".to_string());
        }
    }

    Err("point_select_managed_surface_unavailable".into())
}

fn begin_script(request_token: &str, route: &str) -> Result<String, String> {
    let token = serde_json::to_string(request_token).map_err(|_| "point_select_token_invalid")?;
    let route = serde_json::to_string(route).map_err(|_| "point_select_route_invalid")?;
    Ok(format!(
        "(() => {{ const api = window.__LOCALVIEW__; if (!api?.beginPointSelect) return false; return api.beginPointSelect({{ requestToken: {token}, route: {route} }}); }})()"
    ))
}

fn cancel_script(request_token: &str) -> Result<String, String> {
    let token = serde_json::to_string(request_token).map_err(|_| "point_select_token_invalid")?;
    Ok(format!(
        "window.__LOCALVIEW__?.cancelPointSelect?.({token}, 'cancelled');"
    ))
}

#[tauri::command]
pub async fn point_select_begin(
    app: tauri::AppHandle,
    state: tauri::State<'_, PointSelectState>,
    session_id: SessionId,
    request_token: String,
) -> Result<HumanPointSelectStatus, String> {
    validate_point_select_token(&request_token)?;
    let route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    let route = canonical_point_select_route(&route)?;
    let status = state
        .begin(session_id, request_token.clone(), route.clone())
        .await?;
    let script = begin_script(&request_token, &route)?;
    if let Err(error) = managed_point_select_eval(&app, session_id, &script) {
        let _ = state
            .cancel(session_id, &request_token, "runtime_unavailable")
            .await;
        return Err(error);
    }
    Ok(status)
}

#[tauri::command]
pub async fn point_select_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, PointSelectState>,
    session_id: SessionId,
    request_token: String,
) -> Result<HumanPointSelectStatus, String> {
    validate_point_select_token(&request_token)?;
    let current_route = visual_capture::managed_surface_canonical_route(&app, session_id)
        .and_then(|route| canonical_point_select_route(&route));
    let status = state
        .status(session_id, &request_token, current_route)
        .await;
    if status.state == HumanPointSelectPhase::Failed {
        let _ = managed_point_select_eval(&app, session_id, &cancel_script(&request_token)?);
    }
    Ok(status)
}

#[tauri::command]
pub async fn point_select_cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, PointSelectState>,
    session_id: SessionId,
    request_token: String,
) -> Result<HumanPointSelectStatus, String> {
    validate_point_select_token(&request_token)?;
    let status = state
        .cancel(session_id, &request_token, "cancelled")
        .await;
    if status.state != HumanPointSelectPhase::Stale {
        let _ = managed_point_select_eval(&app, session_id, &cancel_script(&request_token)?);
    }
    Ok(status)
}

#[tauri::command]
pub async fn preview_complete_point_select(
    webview_window: tauri::WebviewWindow,
    state: tauri::State<'_, PointSelectState>,
    session_id: SessionId,
    completion: PreviewPointSelectCompletion,
) -> Result<(), String> {
    if !workspace_surface::bridge_surface_label_allowed(webview_window.label(), session_id) {
        return Err("point_select_bridge_session_window_mismatch".into());
    }
    let caller_url = webview_window.url().map_err(|_| "point_select_route_unavailable")?;
    if !workspace_surface::workspace_navigation_allowed(&caller_url) {
        return Err("point_select_route_not_loopback".into());
    }
    let caller_route = canonical_point_select_route(caller_url.as_str())?;
    state
        .complete(session_id, &caller_route, completion)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(seed: u128) -> SessionId {
        uuid::Uuid::from_u128(seed)
    }

    fn selected(token: &str, route: &str, reference: &str, generation: u64) -> PreviewPointSelectCompletion {
        PreviewPointSelectCompletion {
            request_token: token.to_owned(),
            route: route.to_owned(),
            status: "selected".to_owned(),
            reference: Some(reference.to_owned()),
            reason: None,
            bridge_generation: generation,
        }
    }

    #[test]
    fn element_reference_validation_is_fail_closed() {
        assert!(valid_element_reference("@e1a2b3c"));
        assert!(!valid_element_reference("button.primary"));
        assert!(!valid_element_reference("@e"));
        assert!(!valid_element_reference("@e1/path"));
        assert!(!valid_element_reference("@eXYZ"));
    }

    #[test]
    fn completion_contract_has_no_private_value_fields() {
        let value = serde_json::to_value(PreviewPointSelectCompletion {
            request_token: "point-safe".into(),
            route: "http://127.0.0.1:5173/".into(),
            status: "selected".into(),
            reference: Some("@e1234".into()),
            reason: None,
            bridge_generation: 7,
        })
        .expect("serialize completion");
        let object = value.as_object().expect("completion object");
        for forbidden in [
            "innerHTML",
            "textContent",
            "value",
            "password",
            "attributes",
            "props",
            "state",
            "hooks",
            "cookies",
            "storage",
        ] {
            assert!(!object.contains_key(forbidden));
        }
    }

    #[tokio::test]
    async fn newer_request_wins_over_stale_completion() {
        let state = PointSelectState::default();
        let session_id = session(1);
        let route = "http://127.0.0.1:5173/".to_string();
        state.begin(session_id, "point-A".into(), route.clone()).await.unwrap();
        state.begin(session_id, "point-B".into(), route.clone()).await.unwrap();

        state
            .complete(
                session_id,
                &route,
                selected("point-A", &route, "@eaaaa", 10),
            )
            .await
            .unwrap();

        let status = state.status(session_id, "point-B", Ok(route.clone())).await;
        assert_eq!(status.state, HumanPointSelectPhase::Pending);
        assert_eq!(status.request_token, "point-B");
        assert!(status.reference.is_none());

        state
            .complete(
                session_id,
                &route,
                selected("point-B", &route, "@ebbbb", 11),
            )
            .await
            .unwrap();
        let status = state.status(session_id, "point-B", Ok(route)).await;
        assert_eq!(status.state, HumanPointSelectPhase::Selected);
        assert_eq!(status.reference.as_deref(), Some("@ebbbb"));
        assert_eq!(status.bridge_generation, Some(11));
    }

    #[tokio::test]
    async fn route_and_session_drift_fail_closed() {
        let state = PointSelectState::default();
        let first = session(2);
        let second = session(3);
        let route = "http://127.0.0.1:5173/a".to_string();
        state.begin(first, "point-route".into(), route.clone()).await.unwrap();

        let drifted = state
            .status(
                first,
                "point-route",
                Ok("http://127.0.0.1:5173/b".into()),
            )
            .await;
        assert_eq!(drifted.state, HumanPointSelectPhase::Failed);
        assert_eq!(drifted.reason, Some("route_changed"));

        let foreign = state
            .status(second, "point-route", Ok(route))
            .await;
        assert_eq!(foreign.state, HumanPointSelectPhase::Stale);
        assert!(foreign.reference.is_none());
    }

    #[tokio::test]
    async fn invalid_reference_never_becomes_selection_authority() {
        let state = PointSelectState::default();
        let session_id = session(4);
        let route = "http://127.0.0.1:5173/".to_string();
        state
            .begin(session_id, "point-invalid".into(), route.clone())
            .await
            .unwrap();

        state
            .complete(
                session_id,
                &route,
                selected("point-invalid", &route, "div:nth-child(2)", 12),
            )
            .await
            .unwrap();
        let status = state
            .status(session_id, "point-invalid", Ok(route))
            .await;
        assert_eq!(status.state, HumanPointSelectPhase::Failed);
        assert_eq!(status.reason, Some("invalid_reference"));
        assert!(status.reference.is_none());
    }
}
