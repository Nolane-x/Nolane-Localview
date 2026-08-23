#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::Capability;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Observe,
    Interact,
    Capture,
    InjectFailure,
    MutateLocalState,
    MutateSource,
    ApplyCandidate,
    LaunchChromium,
    ExternalNetwork,
    ExternalSideEffect,
    ExportEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectPolicy {
    pub allowed: BTreeSet<Permission>,
    pub denied_routes: Vec<String>,
    pub protected_selectors: Vec<String>,
    pub allowed_external_hosts: BTreeSet<String>,
    pub require_confirmation_for: BTreeSet<IntentClass>,
    pub redact_selectors: Vec<String>,
}

impl Default for ProjectPolicy {
    fn default() -> Self {
        Self {
            allowed: BTreeSet::from([Permission::Observe, Permission::Interact, Permission::Capture]),
            denied_routes: Vec::new(),
            protected_selectors: Vec::new(),
            allowed_external_hosts: BTreeSet::new(),
            require_confirmation_for: BTreeSet::from([IntentClass::Destructive, IntentClass::ExternalSideEffect]),
            redact_selectors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum IntentClass { ReadOnly, LocalInteraction, LocalMutation, Destructive, ExternalSideEffect }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionIntent {
    pub name: String,
    pub class: IntentClass,
    pub permission: Permission,
    pub route: Option<String>,
    pub selector: Option<String>,
    pub external_host: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision { Allow, Deny, RequireConfirmation }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyResult { pub decision: PolicyDecision, pub reasons: Vec<String> }

pub fn authorize(intent: &ActionIntent, policy: &ProjectPolicy) -> PolicyResult {
    let mut reasons = Vec::new();
    if !policy.allowed.contains(&intent.permission) { reasons.push(format!("permission {:?} is not granted", intent.permission)); }
    if intent.route.as_ref().is_some_and(|route| policy.denied_routes.iter().any(|prefix| route.starts_with(prefix))) { reasons.push("route is denied by project policy".into()); }
    if intent.selector.as_ref().is_some_and(|selector| policy.protected_selectors.iter().any(|protected| selector == protected)) { reasons.push("target is protected by project policy".into()); }
    if let Some(host) = &intent.external_host {
        if !policy.allowed_external_hosts.contains(host) { reasons.push(format!("external host {host} is not allowlisted")); }
    }
    if !reasons.is_empty() { return PolicyResult { decision: PolicyDecision::Deny, reasons }; }
    if policy.require_confirmation_for.contains(&intent.class) { return PolicyResult { decision: PolicyDecision::RequireConfirmation, reasons: vec!["intent class requires explicit confirmation".into()] }; }
    PolicyResult { decision: PolicyDecision::Allow, reasons }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProfile {
    pub id: String,
    pub capabilities: BTreeSet<Capability>,
    pub permissions: BTreeSet<Permission>,
    pub max_parallel_actions: usize,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionProfile {
    pub id: String,
    pub viewport: String,
    pub locale: String,
    pub theme: String,
    pub reduced_motion: bool,
    pub network_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Persona {
    pub id: String,
    pub locale: String,
    pub direction: TextDirection,
    pub viewport: String,
    pub reduced_motion: bool,
    pub input_mode: InputMode,
    pub runtime_state: BTreeMap<String, String>,
    pub secret_fields: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection { Ltr, Rtl }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputMode { Mouse, Touch, Keyboard, Mixed }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaDrift {
    pub changed_fields: Vec<String>,
    pub security_sensitive: bool,
}

pub fn persona_drift(before: &Persona, after: &Persona) -> PersonaDrift {
    let mut changed = Vec::new();
    if before.locale != after.locale { changed.push("locale".into()); }
    if before.direction != after.direction { changed.push("direction".into()); }
    if before.viewport != after.viewport { changed.push("viewport".into()); }
    if before.reduced_motion != after.reduced_motion { changed.push("reduced_motion".into()); }
    if before.input_mode != after.input_mode { changed.push("input_mode".into()); }
    if before.runtime_state != after.runtime_state { changed.push("runtime_state".into()); }
    let security_sensitive = before.secret_fields != after.secret_fields || changed.iter().any(|field| after.secret_fields.contains(field));
    PersonaDrift { changed_fields: changed, security_sensitive }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrust { BuiltIn, SignedLocal, SignedThirdParty, Untrusted }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub trust: PluginTrust,
    pub requested_permissions: BTreeSet<Permission>,
    pub analyzer_only: bool,
    pub network_access: bool,
}

pub fn plugin_allowed(manifest: &PluginManifest, policy: &ProjectPolicy) -> PolicyResult {
    let ungranted = manifest.requested_permissions.difference(&policy.allowed).copied().collect::<Vec<_>>();
    if !ungranted.is_empty() { return PolicyResult { decision: PolicyDecision::Deny, reasons: vec![format!("plugin requests ungranted permissions: {ungranted:?}")] }; }
    if manifest.trust == PluginTrust::Untrusted && (!manifest.analyzer_only || manifest.network_access) { return PolicyResult { decision: PolicyDecision::Deny, reasons: vec!["untrusted plugins must be analyzer-only and offline".into()] }; }
    PolicyResult { decision: PolicyDecision::Allow, reasons: Vec::new() }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FormAction {
    pub selector: String,
    pub submit: bool,
    pub changes_server_state: bool,
    pub external_destination: Option<String>,
}

pub fn classify_form_action(action: &FormAction) -> IntentClass {
    if action.external_destination.is_some() { IntentClass::ExternalSideEffect }
    else if action.submit && action.changes_server_state { IntentClass::Destructive }
    else if action.submit { IntentClass::LocalMutation }
    else { IntentClass::LocalInteraction }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_host_is_denied_without_allowlist() {
        let policy = ProjectPolicy::default();
        let result = authorize(&ActionIntent { name: "post".into(), class: IntentClass::ExternalSideEffect, permission: Permission::ExternalNetwork, route: None, selector: None, external_host: Some("api.example.com".into()) }, &policy);
        assert_eq!(result.decision, PolicyDecision::Deny);
    }

    #[test]
    fn untrusted_plugin_cannot_request network_or_mutation() {
        let policy = ProjectPolicy { allowed: BTreeSet::from([Permission::Observe]), ..ProjectPolicy::default() };
        let result = plugin_allowed(&PluginManifest { id: "x".into(), trust: PluginTrust::Untrusted, requested_permissions: BTreeSet::from([Permission::Observe]), analyzer_only: true, network_access: true }, &policy);
        assert_eq!(result.decision, PolicyDecision::Deny);
    }
}