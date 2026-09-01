use std::collections::BTreeMap;

use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
    SnapshotPublishError, SnapshotResourceUsage,
};
use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, TargetIncarnationRef,
};

fn provider() -> ProviderIncarnationRef {
    ProviderIncarnationRef::from("provider:windows-uia:worker-1")
}

fn target() -> TargetIncarnationRef {
    TargetIncarnationRef::from("target:windows:selection=one")
}

fn node(name: &str, cut: &str) -> NativeSemanticNodeObservation {
    NativeSemanticNodeObservation {
        element_ref: ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: provider(),
            target_incarnation_ref: target(),
            opaque_provider_element_id: "uia-runtime:[42,7]".into(),
            semantic_locator_hints: vec!["automation_id=save".into()],
            parent_surface_ref: Some("surface:window:1234".into()),
            acquisition_cut_ref: cut.into(),
            realization: ProviderElementRealization::RealizedCurrent,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        },
        parent_index: None,
        depth: 0,
        role: Some("button".into()),
        name: Some(name.into()),
        control_type: Some("button".into()),
        automation_id: Some("save".into()),
        class_name: Some("Button".into()),
        is_enabled: Some(true),
        is_offscreen: Some(false),
        attributes: BTreeMap::new(),
    }
}

fn complete_usage() -> SnapshotResourceUsage {
    SnapshotResourceUsage {
        nodes_observed: 1,
        properties_read: 8,
        max_depth_observed: 0,
        exhausted: vec![],
        incomplete: false,
    }
}

fn draft(name: &str, sequence: u64, cut: &str) -> NativeSemanticSnapshotDraft {
    NativeSemanticSnapshotDraft {
        provider_incarnation_ref: provider(),
        target_incarnation_ref: target(),
        snapshot_cut_ref: cut.into(),
        surface_scope: "window:1234".into(),
        cache_profile_revision: "windows-uia-cache-v1".into(),
        permission_visibility_revision: "uia-permission:interactive-user:v1".into(),
        capture_sequence: sequence,
        nodes: vec![node(name, cut)],
        resource_usage: complete_usage(),
        completeness: ReconciliationCompleteness::Established,
        incompleteness_debt: vec![],
    }
}

#[test]
fn publishing_refresh_creates_a_new_immutable_cache_revision() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());

    let first = cache.publish(draft("Save", 1, "cut:1")).unwrap();
    let second = cache.publish(draft("Saved", 2, "cut:2")).unwrap();

    assert_ne!(first.cache_revision_ref(), second.cache_revision_ref());
    assert_eq!(first.capture_sequence(), 1);
    assert_eq!(first.nodes()[0].name.as_deref(), Some("Save"));
    assert_eq!(second.nodes()[0].name.as_deref(), Some("Saved"));
    assert_eq!(cache.current().unwrap().cache_revision_ref(), second.cache_revision_ref());
}

#[test]
fn cache_lineage_rejects_silent_provider_or_target_reincarnation() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    cache.publish(draft("Save", 1, "cut:1")).unwrap();

    let mut wrong_provider = draft("Save", 2, "cut:2");
    wrong_provider.provider_incarnation_ref = ProviderIncarnationRef::from("provider:windows-uia:worker-2");
    assert_eq!(
        cache.publish(wrong_provider).unwrap_err(),
        SnapshotPublishError::ProviderIncarnationMismatch
    );

    let mut wrong_target = draft("Save", 2, "cut:2");
    wrong_target.target_incarnation_ref = TargetIncarnationRef::from("target:windows:selection=two");
    assert_eq!(
        cache.publish(wrong_target).unwrap_err(),
        SnapshotPublishError::TargetIncarnationMismatch
    );
}

#[test]
fn incomplete_observation_cannot_claim_established_completeness() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    let mut incomplete = draft("Save", 1, "cut:1");
    incomplete.resource_usage.incomplete = true;
    incomplete.incompleteness_debt = vec!["snapshot_node_budget_exhausted".into()];

    assert_eq!(
        cache.publish(incomplete).unwrap_err(),
        SnapshotPublishError::EstablishedSnapshotHasIncompleteness
    );
}

#[test]
fn cache_rejects_non_monotonic_capture_sequence() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    cache.publish(draft("Save", 2, "cut:2")).unwrap();

    assert_eq!(
        cache.publish(draft("Old", 1, "cut:1")).unwrap_err(),
        SnapshotPublishError::NonMonotonicCaptureSequence
    );
    assert_eq!(
        cache.publish(draft("Duplicate", 2, "cut:2b")).unwrap_err(),
        SnapshotPublishError::NonMonotonicCaptureSequence
    );
}

#[test]
fn mixed_cut_nodes_cannot_be_published_as_one_snapshot() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    let mut mixed = draft("Save", 1, "cut:1");
    mixed.nodes[0].element_ref.acquisition_cut_ref = "cut:older".into();

    assert_eq!(
        cache.publish(mixed).unwrap_err(),
        SnapshotPublishError::MixedObservationCut
    );
}

#[test]
fn reconciliation_receipt_is_an_exact_projection_of_the_snapshot_revision() {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    let snapshot = cache.publish(draft("Save", 7, "cut:7")).unwrap();

    let receipt = snapshot.reconciliation_receipt("reconcile:7");
    assert_eq!(receipt.receipt_id, "reconcile:7");
    assert_eq!(receipt.provider_incarnation_ref, provider());
    assert_eq!(receipt.target_incarnation_ref, target());
    assert_eq!(receipt.snapshot_cut_ref, "cut:7");
    assert_eq!(receipt.surface_scope, "window:1234");
    assert_eq!(receipt.completeness, ReconciliationCompleteness::Established);
    assert_eq!(receipt.cache_profile_revision, "windows-uia-cache-v1");
    assert_eq!(receipt.permission_visibility_revision, "uia-permission:interactive-user:v1");
    assert_eq!(receipt.capture_sequence, 7);
    assert_eq!(receipt.observed_digest, snapshot.observed_digest());
    assert!(receipt.incompleteness_debt.is_empty());
}
