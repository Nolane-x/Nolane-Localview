#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use localview_content_addressed::object_hash;
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, ReconciliationSnapshotReceipt, TargetIncarnationRef,
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

/// A provider-normalized semantic observation. It deliberately contains no live
/// UIA/COM object: provider-owned handles stay inside the OS worker apartment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSemanticNodeObservation {
    pub element_ref: ProviderElementRef,
    pub parent_index: Option<usize>,
    pub depth: usize,
    pub role: Option<String>,
    pub name: Option<String>,
    pub control_type: Option<String>,
    pub automation_id: Option<String>,
    pub class_name: Option<String>,
    pub is_enabled: Option<bool>,
    pub is_offscreen: Option<bool>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

/// Mutable construction payload owned only by the observation transaction. Once
/// published it is converted into an immutable snapshot revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSemanticSnapshotDraft {
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub snapshot_cut_ref: String,
    pub surface_scope: String,
    pub cache_profile_revision: String,
    pub permission_visibility_revision: String,
    pub capture_sequence: u64,
    pub nodes: Vec<NativeSemanticNodeObservation>,
    pub resource_usage: SnapshotResourceUsage,
    pub completeness: ReconciliationCompleteness,
    #[serde(default)]
    pub incompleteness_debt: Vec<String>,
}

#[derive(Debug, Clone, Copy, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotPublishError {
    #[error("snapshot provider incarnation does not match cache lineage")]
    ProviderIncarnationMismatch,
    #[error("snapshot target incarnation does not match cache lineage")]
    TargetIncarnationMismatch,
    #[error("established snapshot contains incompleteness")]
    EstablishedSnapshotHasIncompleteness,
    #[error("snapshot resource usage is internally inconsistent")]
    ResourceUsageInconsistent,
    #[error("snapshot cut is missing")]
    MissingSnapshotCut,
    #[error("snapshot surface scope is missing")]
    MissingSurfaceScope,
    #[error("snapshot cache profile revision is missing")]
    MissingCacheProfileRevision,
    #[error("snapshot permission visibility revision is missing")]
    MissingPermissionVisibilityRevision,
    #[error("snapshot capture sequence did not advance")]
    NonMonotonicCaptureSequence,
    #[error("node provider incarnation does not match snapshot")]
    NodeProviderIncarnationMismatch,
    #[error("node target incarnation does not match snapshot")]
    NodeTargetIncarnationMismatch,
    #[error("node acquisition cut does not match snapshot")]
    MixedObservationCut,
    #[error("node parent reference is not a prior node in this revision")]
    InvalidNodeParent,
}

/// An immutable provider observation revision. All fields are private and only
/// read-only accessors are exposed, so refreshing a cache can never mutate the
/// evidence held by an older revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeSemanticSnapshotRevision {
    cache_revision_ref: String,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    snapshot_cut_ref: String,
    surface_scope: String,
    cache_profile_revision: String,
    permission_visibility_revision: String,
    capture_sequence: u64,
    nodes: Vec<NativeSemanticNodeObservation>,
    resource_usage: SnapshotResourceUsage,
    completeness: ReconciliationCompleteness,
    observed_digest: String,
    incompleteness_debt: Vec<String>,
}

impl NativeSemanticSnapshotRevision {
    pub fn cache_revision_ref(&self) -> &str {
        &self.cache_revision_ref
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn capture_sequence(&self) -> u64 {
        self.capture_sequence
    }

    pub fn nodes(&self) -> &[NativeSemanticNodeObservation] {
        &self.nodes
    }

    pub fn resource_usage(&self) -> &SnapshotResourceUsage {
        &self.resource_usage
    }

    pub fn completeness(&self) -> ReconciliationCompleteness {
        self.completeness
    }

    pub fn observed_digest(&self) -> &str {
        &self.observed_digest
    }

    pub fn incompleteness_debt(&self) -> &[String] {
        &self.incompleteness_debt
    }

    /// Project this exact immutable revision into the protocol receipt. No
    /// completeness inference is performed here: the published revision is the
    /// authority for the receipt fields.
    pub fn reconciliation_receipt(
        &self,
        receipt_id: impl Into<String>,
    ) -> ReconciliationSnapshotReceipt {
        ReconciliationSnapshotReceipt {
            receipt_id: receipt_id.into(),
            provider_incarnation_ref: self.provider_incarnation_ref.clone(),
            target_incarnation_ref: self.target_incarnation_ref.clone(),
            snapshot_cut_ref: self.snapshot_cut_ref.clone(),
            surface_scope: self.surface_scope.clone(),
            completeness: self.completeness,
            cache_profile_revision: self.cache_profile_revision.clone(),
            permission_visibility_revision: self.permission_visibility_revision.clone(),
            capture_sequence: self.capture_sequence,
            observed_digest: self.observed_digest.clone(),
            incompleteness_debt: self.incompleteness_debt.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SemanticSnapshotCache {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    current: Option<Arc<NativeSemanticSnapshotRevision>>,
}

impl SemanticSnapshotCache {
    pub fn for_lineage(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
    ) -> Self {
        Self {
            provider_incarnation_ref,
            target_incarnation_ref,
            current: None,
        }
    }

    pub fn current(&self) -> Option<Arc<NativeSemanticSnapshotRevision>> {
        self.current.clone()
    }

    pub fn publish(
        &mut self,
        draft: NativeSemanticSnapshotDraft,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, SnapshotPublishError> {
        self.validate(&draft)?;

        let observed_digest = object_hash(&draft);
        let cache_revision_ref = format!(
            "native-semantic-cache:{}:{}",
            draft.capture_sequence, observed_digest
        );
        let revision = Arc::new(NativeSemanticSnapshotRevision {
            cache_revision_ref,
            provider_incarnation_ref: draft.provider_incarnation_ref,
            target_incarnation_ref: draft.target_incarnation_ref,
            snapshot_cut_ref: draft.snapshot_cut_ref,
            surface_scope: draft.surface_scope,
            cache_profile_revision: draft.cache_profile_revision,
            permission_visibility_revision: draft.permission_visibility_revision,
            capture_sequence: draft.capture_sequence,
            nodes: draft.nodes,
            resource_usage: draft.resource_usage,
            completeness: draft.completeness,
            observed_digest,
            incompleteness_debt: draft.incompleteness_debt,
        });
        self.current = Some(revision.clone());
        Ok(revision)
    }

    fn validate(&self, draft: &NativeSemanticSnapshotDraft) -> Result<(), SnapshotPublishError> {
        if draft.provider_incarnation_ref != self.provider_incarnation_ref {
            return Err(SnapshotPublishError::ProviderIncarnationMismatch);
        }
        if draft.target_incarnation_ref != self.target_incarnation_ref {
            return Err(SnapshotPublishError::TargetIncarnationMismatch);
        }
        if draft.snapshot_cut_ref.trim().is_empty() {
            return Err(SnapshotPublishError::MissingSnapshotCut);
        }
        if draft.surface_scope.trim().is_empty() {
            return Err(SnapshotPublishError::MissingSurfaceScope);
        }
        if draft.cache_profile_revision.trim().is_empty() {
            return Err(SnapshotPublishError::MissingCacheProfileRevision);
        }
        if draft.permission_visibility_revision.trim().is_empty() {
            return Err(SnapshotPublishError::MissingPermissionVisibilityRevision);
        }
        if let Some(current) = &self.current {
            if draft.capture_sequence <= current.capture_sequence {
                return Err(SnapshotPublishError::NonMonotonicCaptureSequence);
            }
        }

        let usage_incomplete = draft.resource_usage.incomplete
            || !draft.resource_usage.exhausted.is_empty()
            || !draft.incompleteness_debt.is_empty();
        if draft.completeness == ReconciliationCompleteness::Established && usage_incomplete {
            return Err(SnapshotPublishError::EstablishedSnapshotHasIncompleteness);
        }
        if draft.resource_usage.incomplete != !draft.resource_usage.exhausted.is_empty() {
            return Err(SnapshotPublishError::ResourceUsageInconsistent);
        }
        if draft.resource_usage.nodes_observed != draft.nodes.len() {
            return Err(SnapshotPublishError::ResourceUsageInconsistent);
        }

        for (index, node) in draft.nodes.iter().enumerate() {
            if node.element_ref.provider_incarnation_ref != draft.provider_incarnation_ref {
                return Err(SnapshotPublishError::NodeProviderIncarnationMismatch);
            }
            if node.element_ref.target_incarnation_ref != draft.target_incarnation_ref {
                return Err(SnapshotPublishError::NodeTargetIncarnationMismatch);
            }
            if node.element_ref.acquisition_cut_ref != draft.snapshot_cut_ref {
                return Err(SnapshotPublishError::MixedObservationCut);
            }
            if node.parent_index.is_some_and(|parent| parent >= index) {
                return Err(SnapshotPublishError::InvalidNodeParent);
            }
        }

        Ok(())
    }
}
