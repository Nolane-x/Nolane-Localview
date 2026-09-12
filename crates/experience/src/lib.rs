#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoOpenPreference { Never, FirstSession, FrontendOnly, Always }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectPreferences {
    pub auto_open: AutoOpenPreference,
    pub remember_window: bool,
    pub xray_default: bool,
    pub preferred_viewport: Option<String>,
    pub notifications: NotificationPreference,
}

impl Default for ProjectPreferences {
    fn default() -> Self {
        Self { auto_open: AutoOpenPreference::FirstSession, remember_window: true, xray_default: false, preferred_viewport: None, notifications: NotificationPreference::ImportantOnly }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPreference { Off, ImportantOnly, All }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoOpenContext {
    pub is_frontend: bool,
    pub first_session_for_project: bool,
    pub user_active_in_localview: bool,
    pub existing_preview_window: bool,
    pub runtime_paused: bool,
    pub explicit_open_request: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenDecision { StayHidden, FocusExisting, OpenNew }

pub fn open_decision(context: &AutoOpenContext, preference: AutoOpenPreference) -> OpenDecision {
    if context.runtime_paused && !context.explicit_open_request { return OpenDecision::StayHidden; }
    if context.existing_preview_window && (context.explicit_open_request || context.user_active_in_localview) { return OpenDecision::FocusExisting; }
    if context.explicit_open_request { return OpenDecision::OpenNew; }
    match preference {
        AutoOpenPreference::Never => OpenDecision::StayHidden,
        AutoOpenPreference::FirstSession if context.first_session_for_project && context.is_frontend => OpenDecision::OpenNew,
        AutoOpenPreference::FrontendOnly if context.is_frontend && !context.user_active_in_localview => OpenDecision::OpenNew,
        AutoOpenPreference::Always => OpenDecision::OpenNew,
        _ => OpenDecision::StayHidden,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EventImportance { Background, Informational, Warning, ActionRequired }

pub fn should_notify(preference: NotificationPreference, importance: EventImportance, app_focused: bool) -> bool {
    if app_focused && importance < EventImportance::ActionRequired { return false; }
    match preference {
        NotificationPreference::Off => false,
        NotificationPreference::ImportantOnly => importance >= EventImportance::Warning,
        NotificationPreference::All => importance >= EventImportance::Informational,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepLink {
    pub session_id: Option<String>,
    pub route: Option<String>,
    pub element_ref: Option<String>,
    pub panel: Option<String>,
}

pub fn parse_deep_link(input: &str) -> Result<DeepLink, url::ParseError> {
    let url = Url::parse(input)?;
    let values = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
    Ok(DeepLink {
        session_id: values.get("session").cloned(),
        route: values.get("route").cloned(),
        element_ref: values.get("ref").cloned(),
        panel: values.get("panel").cloned(),
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrayAction { OpenWorkspace, OpenLastSession, PauseDetection, ResumeDetection, CopyControlStatus, Quit }

pub fn tray_actions(paused: bool, has_session: bool) -> Vec<TrayAction> {
    let mut actions = vec![TrayAction::OpenWorkspace];
    if has_session { actions.push(TrayAction::OpenLastSession); }
    actions.push(if paused { TrayAction::ResumeDetection } else { TrayAction::PauseDetection });
    actions.push(TrayAction::CopyControlStatus);
    actions.push(TrayAction::Quit);
    actions
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingState {
    pub daemon_ready: bool,
    pub frontend_detected: bool,
    pub preview_opened: bool,
    pub agent_connected: bool,
}

impl OnboardingState {
    pub fn next_hint(&self) -> Option<&'static str> {
        if !self.daemon_ready { Some("Start the LocalView runtime") }
        else if !self.frontend_detected { Some("Run a local frontend dev server") }
        else if !self.preview_opened { Some("Open the detected preview") }
        else if !self.agent_connected { Some("Optional: connect CLI or MCP") }
        else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_window_is_focused_instead_of_spawned() {
        let context = AutoOpenContext { is_frontend: true, first_session_for_project: false, user_active_in_localview: true, existing_preview_window: true, runtime_paused: false, explicit_open_request: true };
        assert_eq!(open_decision(&context, AutoOpenPreference::Always), OpenDecision::FocusExisting);
    }

    #[test]
    fn focused_app_suppresses_background_notifications() {
        assert!(!should_notify(NotificationPreference::All, EventImportance::Informational, true));
    }
}