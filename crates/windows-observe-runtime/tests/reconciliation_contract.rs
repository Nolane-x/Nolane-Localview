use chrono::{TimeZone, Utc};
use localview_live_bridge::{LiveBridge, ObserverEventKind};
use localview_native_provider::{
    NativeSemanticSnapshotDraft, SemanticSnapshotCache, SnapshotResourceUsage,
};
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness, SessionId,
    TargetIncarnationRef,
};
use localview_windows_observe_runtime::WindowsObserveBridgeBinding;
use localview_windows_uia_provider::{
    WindowsUiaEvent, WindowsUiaEventDrain, WindowsUiaEventKind,
};
use uuid::Uuid;

fn provider() -> ProviderIncarnationRef {
    ProviderIncarnationRef::from("provider:windows-uia:runtime-contract")
}

fn target() -> TargetIncarnationRef {
    TargetIncarnationRef::from("target:windows:selection=runtime-contract")
}

fn session() -> SessionId {
    Uuid::from_u128(0x43)
}

fn snapshot(sequence: u64) -> std::sync::Arc<localview_native_provider::NativeSemanticSnapshotRevision> {
    let mut cache = SemanticSnapshotCache::for_lineage(provider(), target());
    cache
        .publish(NativeSemanticSnapshotDraft {
            provider_incarnation_ref: provider(),
            target_incarnation_ref: target(),
            snapshot_cut_ref: format!("uia-reconcile-cut:{sequence}"),
            surface_scope: "window:runtime-contract".into(),
            cache_profile_revision: "windows-uia-cache-v1".into(),
            permission_visibility_revision: "uia-permission:interactive-user:v1".into(),
            capture_sequence: sequence,
            nodes: vec![],
            resource_usage: SnapshotResourceUsage {
                nodes_observed: 0,
                properties_read: 0,
                max_depth_observed: 0,
                exhausted: vec![],
                incomplete: false,
            },
            completeness: ReconciliationCompleteness::Established,
            incompleteness_debt: vec![],
        })
        .unwrap()
}

#[tokio::test]
async fn binding_starts_windows_uia_at_opaque_ordering_and_exact_sequence_baseline() {
    let bridge = LiveBridge::new(64, 8);
    let binding = WindowsObserveBridgeBinding::new(session(), 7, provider(), target(), 41);

    let status = binding.bind(&bridge).await.unwrap();

    assert_eq!(status.generation, 7);
    assert_eq!(status.last_seq, Some(41));
    assert_eq!(status.event_continuity, EventContinuityState::OrderingOpaque);
    assert_eq!(status.provider_incarnation_ref, provider());
    assert_eq!(status.target_incarnation_ref, target());
}

#[tokio::test]
async fn bounded_buffer_drop_becomes_explicit_livebridge_gap_and_preserves_callback_time() {
    let bridge = LiveBridge::new(64, 8);
    let binding = WindowsObserveBridgeBinding::new(session(), 7, provider(), target(), 0);
    binding.bind(&bridge).await.unwrap();
    let captured_at = Utc.with_ymd_and_hms(2026, 9, 1, 15, 0, 0).unwrap();

    let report = binding
        .ingest_drain(
            &bridge,
            WindowsUiaEventDrain {
                events: vec![WindowsUiaEvent {
                    sequence: 3,
                    captured_at,
                    provider_incarnation_ref: provider(),
                    target_incarnation_ref: target(),
                    kind: WindowsUiaEventKind::PropertyChanged { property_id: 30005 },
                    element_ref: None,
                }],
                dropped_before_drain: 2,
                latest_sequence: 3,
            },
        )
        .await
        .unwrap();

    assert_eq!(report.continuity, EventContinuityState::GapDetected);
    let gap = report.gap.expect("dropped UIA events must become a continuity gap");
    assert_eq!(gap.expected_sequence, 1);
    assert_eq!(gap.observed_sequence, 3);

    let recent = bridge.recent(session(), 8).await;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].captured_at, captured_at);
    assert_eq!(recent[0].kind, ObserverEventKind::SemanticSnapshot);
    assert_eq!(recent[0].payload["native_provider"], "windows_uia");
    assert_eq!(recent[0].payload["native_event"], "property_changed");
    assert_eq!(recent[0].payload["property_id"], 30005);
    assert_eq!(recent[0].payload["dropped_before_drain"], 2);
}

#[tokio::test]
async fn reconciliation_establishes_current_snapshot_without_laundering_prior_gap() {
    let bridge = LiveBridge::new(64, 8);
    let binding = WindowsObserveBridgeBinding::new(session(), 7, provider(), target(), 0);
    binding.bind(&bridge).await.unwrap();

    binding
        .ingest_drain(
            &bridge,
            WindowsUiaEventDrain {
                events: vec![WindowsUiaEvent {
                    sequence: 4,
                    captured_at: Utc::now(),
                    provider_incarnation_ref: provider(),
                    target_incarnation_ref: target(),
                    kind: WindowsUiaEventKind::StructureChanged { change_type: 2 },
                    element_ref: None,
                }],
                dropped_before_drain: 3,
                latest_sequence: 4,
            },
        )
        .await
        .unwrap();

    let status = binding
        .record_snapshot_reconciliation(&bridge, snapshot(9).as_ref(), "reconcile:windows-uia:9")
        .await
        .unwrap();

    assert_eq!(status.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        status.reconciliation_receipt_id.as_deref(),
        Some("reconcile:windows-uia:9")
    );
    assert!(status.gap.is_some());
}

#[tokio::test]
async fn wrong_lineage_drain_fails_closed_without_relabelling_provider_evidence() {
    let bridge = LiveBridge::new(64, 8);
    let binding = WindowsObserveBridgeBinding::new(session(), 7, provider(), target(), 0);
    binding.bind(&bridge).await.unwrap();

    let error = binding
        .ingest_drain(
            &bridge,
            WindowsUiaEventDrain {
                events: vec![WindowsUiaEvent {
                    sequence: 1,
                    captured_at: Utc::now(),
                    provider_incarnation_ref: ProviderIncarnationRef::from(
                        "provider:windows-uia:other",
                    ),
                    target_incarnation_ref: target(),
                    kind: WindowsUiaEventKind::FocusChanged,
                    element_ref: None,
                }],
                dropped_before_drain: 0,
                latest_sequence: 1,
            },
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("provider incarnation"));
    assert!(bridge.recent(session(), 8).await.is_empty());
}
