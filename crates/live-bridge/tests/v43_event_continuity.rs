use chrono::Utc;
use localview_live_bridge::{
    EventSequenceGap, LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind,
    ProviderObserverBatch,
};
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, ReconciliationCompleteness,
    ReconciliationSnapshotReceipt, TargetIncarnationRef,
};
use serde_json::Value;
use uuid::Uuid;

fn event(seq: u64) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at: Utc::now(),
        kind: ObserverEventKind::DomMutation,
        reference: Some(format!("@e{seq}")),
        route: Some("/".into()),
        payload: Value::Null,
    }
}

fn reconciliation(
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
) -> ReconciliationSnapshotReceipt {
    ReconciliationSnapshotReceipt {
        receipt_id: "reconcile:1".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        snapshot_cut_ref: "cut:9".into(),
        surface_scope: "active_surface".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:v1".into(),
        permission_visibility_revision: "permission:v1".into(),
        capture_sequence: 9,
        observed_digest: "sha256:current".into(),
        incompleteness_debt: Vec::new(),
    }
}

#[tokio::test]
async fn sequence_gap_is_explicit_and_reconciliation_does_not_launder_continuity() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");

    let report = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(1), event(3)],
        })
        .await;

    assert_eq!(report.ingest.accepted, 2);
    assert_eq!(report.continuity, EventContinuityState::GapDetected);
    assert_eq!(
        report.gap,
        Some(EventSequenceGap {
            expected_sequence: 2,
            observed_sequence: 3,
        })
    );

    let before = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(before.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(before.current_snapshot_completeness, None);

    assert!(
        bridge
            .record_reconciliation(session_id, reconciliation(provider, target))
            .await
    );

    let after = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(after.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(
        after.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
}

#[tokio::test]
async fn generation_reconnect_is_not_continuous_by_default() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:stable");
    let target = TargetIncarnationRef::from("target:webview:stable");

    let first = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(10), event(11)],
        })
        .await;
    assert_eq!(first.continuity, EventContinuityState::Continuous);

    let reconnect = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 2,
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            events: vec![event(1)],
        })
        .await;

    assert_eq!(
        reconnect.continuity,
        EventContinuityState::ReconnectedUnreconciled
    );
    assert_eq!(bridge.recent(session_id, 10).await.len(), 1);
}

#[tokio::test]
async fn provider_reincarnation_cannot_resurrect_old_event_lineage() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let target = TargetIncarnationRef::from("target:webview:1");

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:webview:old"),
            target_incarnation_ref: target.clone(),
            events: vec![event(1), event(2)],
        })
        .await;

    let reincarnated = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: ProviderIncarnationRef::from("provider:webview:new"),
            target_incarnation_ref: target,
            events: vec![event(1)],
        })
        .await;

    assert_eq!(
        reincarnated.continuity,
        EventContinuityState::ProviderReincarnated
    );
    assert_eq!(bridge.recent(session_id, 10).await.len(), 1);
}

#[tokio::test]
async fn reconciliation_for_a_different_incarnation_is_rejected() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:1");
    let target = TargetIncarnationRef::from("target:webview:1");

    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider,
            target_incarnation_ref: target.clone(),
            events: vec![event(1)],
        })
        .await;

    assert!(
        !bridge
            .record_reconciliation(
                session_id,
                reconciliation(
                    ProviderIncarnationRef::from("provider:webview:other"),
                    target,
                ),
            )
            .await
    );
    assert_eq!(
        bridge
            .observation_status(session_id)
            .await
            .unwrap()
            .current_snapshot_completeness,
        None
    );
}

#[tokio::test]
async fn legacy_observer_batch_behavior_remains_compatible() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let report = bridge
        .ingest(ObserverBatch {
            session_id,
            generation: 1,
            events: vec![event(1), event(3)],
        })
        .await;

    assert_eq!(report.accepted, 2);
    assert_eq!(report.rejected_stale, 0);
    assert_eq!(bridge.recent(session_id, 10).await.len(), 2);
}
