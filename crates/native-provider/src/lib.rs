#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// An explicit attachment choice made by the user. The nonce deliberately scopes
/// authority to one attachment lifetime so a later re-attachment cannot inherit
/// stale target authority merely because Windows reused an HWND/PID pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSelectedWindowTarget {
    pub native_window_handle: u64,
    pub expected_process_id: u32,
    pub selection_nonce: Uuid,
}

/// Provider-observed Windows lifetime facts used to establish a target
/// incarnation. RuntimeId is retained only as a non-authoritative hint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowsTargetFingerprint {
    pub native_window_handle: u64,
    pub process_id: u32,
    pub process_start_time_ticks: u64,
    #[serde(default)]
    pub root_runtime_id_hint: Vec<i32>,
}

#[derive(Debug, Clone, Copy, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeProviderIdentityError {
    #[error("window selection is invalid")]
    InvalidSelection,
    #[error("observed native window does not match the explicit selection")]
    WindowSelectionMismatch,
    #[error("observed process does not match the explicit selection")]
    ProcessSelectionMismatch,
    #[error("observed process lifetime is invalid")]
    InvalidProcessLifetime,
}

/// Derive a conservative target incarnation from explicit attachment authority
/// plus OS lifetime facts. UIA RuntimeId is intentionally excluded: Windows may
/// reuse it over time and it is not durable identity by itself.
pub fn derive_windows_target_incarnation(
    selection: &UserSelectedWindowTarget,
    fingerprint: &WindowsTargetFingerprint,
) -> Result<TargetIncarnationRef, NativeProviderIdentityError> {
    if selection.native_window_handle == 0
        || selection.expected_process_id == 0
        || selection.selection_nonce.is_nil()
    {
        return Err(NativeProviderIdentityError::InvalidSelection);
    }
    if fingerprint.native_window_handle != selection.native_window_handle {
        return Err(NativeProviderIdentityError::WindowSelectionMismatch);
    }
    if fingerprint.process_id != selection.expected_process_id {
        return Err(NativeProviderIdentityError::ProcessSelectionMismatch);
    }
    if fingerprint.process_start_time_ticks == 0 {
        return Err(NativeProviderIdentityError::InvalidProcessLifetime);
    }

    Ok(TargetIncarnationRef::from(format!(
        "target:windows:selection={}:hwnd={:x}:pid={}:started={}",
        selection.selection_nonce,
        fingerprint.native_window_handle,
        fingerprint.process_id,
        fingerprint.process_start_time_ticks
    )))
}

/// Project one UIA RuntimeId into a provider-local element reference. The opaque
/// ID may repeat after provider/target reincarnation; the surrounding incarnation
/// refs are what prevent it from becoming durable authority.
pub fn provider_element_ref_from_runtime_id(
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    runtime_id: &[i32],
    acquisition_cut_ref: impl Into<String>,
    realization: ProviderElementRealization,
) -> ProviderElementRef {
    let opaque_provider_element_id = if runtime_id.is_empty() {
        "uia-runtime:unavailable".to_owned()
    } else {
        let values = runtime_id
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!("uia-runtime:[{values}]")
    };

    ProviderElementRef {
        provider_family: "windows_uia".into(),
        provider_incarnation_ref,
        target_incarnation_ref,
        opaque_provider_element_id,
        semantic_locator_hints: Vec::new(),
        parent_surface_ref: None,
        acquisition_cut_ref: acquisition_cut_ref.into(),
        realization,
        lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEventOrdering {
    TotalWithinProviderStream,
    PerObjectOnly,
    PerThreadOnly,
    ActionCorrelated,
    OpaqueBestEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderEventReliabilityProfile {
    pub profile_revision: String,
    pub ordering: ProviderEventOrdering,
    pub property_change_events_complete: bool,
    pub structure_change_events_complete: bool,
    pub action_critical_properties_require_reconciliation: bool,
    pub global_polling_required: bool,
}

impl ProviderEventReliabilityProfile {
    pub fn windows_uia_v1() -> Self {
        Self {
            profile_revision: "windows-uia-events-v1".into(),
            ordering: ProviderEventOrdering::OpaqueBestEffort,
            property_change_events_complete: false,
            structure_change_events_complete: false,
            action_critical_properties_require_reconciliation: true,
            global_polling_required: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeProviderCapabilities {
    pub semantic_snapshot: bool,
    pub event_subscription: bool,
    pub reconciliation: bool,
    pub resource_accounting: bool,
    pub write_actions: bool,
    pub input_dispatch: bool,
}

impl NativeProviderCapabilities {
    pub const fn windows_observe_only() -> Self {
        Self {
            semantic_snapshot: true,
            event_subscription: true,
            reconciliation: true,
            resource_accounting: true,
            write_actions: false,
            input_dispatch: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotBudget {
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_properties: usize,
}

impl Default for SnapshotBudget {
    fn default() -> Self {
        Self {
            max_nodes: 512,
            max_depth: 16,
            max_properties: 4096,
        }
    }
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotBudgetLimit {
    Nodes,
    Depth,
    Properties,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotResourceUsage {
    pub nodes_observed: usize,
    pub properties_read: usize,
    pub max_depth_observed: usize,
    pub exhausted: Vec<SnapshotBudgetLimit>,
    pub incomplete: bool,
}

#[derive(Debug, Clone)]
pub struct SnapshotBudgetGuard {
    budget: SnapshotBudget,
    nodes_observed: usize,
    properties_read: usize,
    max_depth_observed: usize,
    exhausted: BTreeSet<SnapshotBudgetLimit>,
}

impl SnapshotBudgetGuard {
    pub fn new(budget: SnapshotBudget) -> Self {
        Self {
            budget,
            nodes_observed: 0,
            properties_read: 0,
            max_depth_observed: 0,
            exhausted: BTreeSet::new(),
        }
    }

    /// Admit one semantic node atomically against node/depth/property bounds.
    /// Rejected work never partially increments usage, but every violated bound
    /// is retained as explicit incompleteness evidence.
    pub fn admit_node(&mut self, depth: usize, properties_to_read: usize) -> bool {
        let mut violated = Vec::new();
        if self.nodes_observed >= self.budget.max_nodes {
            violated.push(SnapshotBudgetLimit::Nodes);
        }
        if depth > self.budget.max_depth {
            violated.push(SnapshotBudgetLimit::Depth);
        }
        if self
            .properties_read
            .saturating_add(properties_to_read)
            > self.budget.max_properties
        {
            violated.push(SnapshotBudgetLimit::Properties);
        }

        if !violated.is_empty() {
            self.exhausted.extend(violated);
            return false;
        }

        self.nodes_observed = self.nodes_observed.saturating_add(1);
        self.properties_read = self.properties_read.saturating_add(properties_to_read);
        self.max_depth_observed = self.max_depth_observed.max(depth);
        true
    }

    pub fn finish(self) -> SnapshotResourceUsage {
        let exhausted = self.exhausted.into_iter().collect::<Vec<_>>();
        SnapshotResourceUsage {
            nodes_observed: self.nodes_observed,
            properties_read: self.properties_read,
            max_depth_observed: self.max_depth_observed,
            incomplete: !exhausted.is_empty(),
            exhausted,
        }
    }
}
