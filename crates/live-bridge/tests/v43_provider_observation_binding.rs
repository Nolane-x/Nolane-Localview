use chrono::Utc;
use localview_live_bridge::{
    EventSequenceGap, LiveBridge, ObserverBatch, ObserverEvent, ObserverEventKind,
    ProviderObservationBinding, ProviderObservationBindingError, ProviderObserverBatch,
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
        kind: ObserverEventKind::SemanticSnapshot,
        reference: None,
        route: None,
        payload: Value::Null,
    }
}

fn receipt(
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
) -> ReconciliationSnapshotReceipt {
    ReconciliationSnapshotReceipt {
        receipt_id: "reconcile:uia:1".into(),
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        snapshot_cut_ref: "cut:uia:1".into(),
        surface_scope: "window:1234".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "windows-uia-control-view-v1".into(),
        permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
        capture_sequence: 1,
        observed_digest: "sha256:uia-current".into(),
        incompleteness_debt: Vec::new(),
    }
}

fn binding(
    session_id: Uuid,
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    continuity: EventContinuityState,
    sequence_baseline: Option<u64>,
) -> ProviderObservationBinding {
    ProviderObservationBinding {
        session_id,
        generation: 1,
        provider_incarnation_ref: provider,
        target_incarnation_ref: target,
        initial_continuity: continuity,
        sequence_baseline,
    }
}

#[tokio::test]
async fn opaque_provider_binding_preserves_reliability_and_detects_initial_buffer_loss() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:windows-uia:mta:1");
    let target = TargetIncarnationRef::from("target:windows:selection=1");

    let bound = bridge
        .bind_provider_observation(binding(
            session_id,
            provider.clone(),
            target.clone(),
            EventContinuityState::OrderingOpaque,
            Some(0),
        ))
        .await
        .unwrap();
    assert_eq!(bound.event_continuity, EventContinuityState::OrderingOpaque);
    assert_eq!(bound.last_seq, Some(0));

    let report = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            events: vec![event(3)],
        })
        .await;

    assert_eq!(report.continuity, EventContinuityState::GapDetected);
    assert_eq!(
        report.gap,
        Some(EventSequenceGap {
            expected_sequence: 1,
            observed_sequence: 3,
        })
    );
}

#[tokio::test]
async fn reconciliation_establishes_current_snapshot_without_upgrading_opaque_event_ordering() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:windows-uia:mta:2");
    let target = TargetIncarnationRef::from("target:windows:selection=2");

    bridge
        .bind_provider_observation(binding(
            session_id,
            provider.clone(),
            target.clone(),
            EventContinuityState::OrderingOpaque,
            Some(0),
        ))
        .await
        .unwrap();
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(1)],
        })
        .await;

    assert!(bridge.record_reconciliation(session_id, receipt(provider, target)).await);
    let status = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(status.event_continuity, EventContinuityState::OrderingOpaque);
    assert_eq!(
        status.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
}

#[tokio::test]
async fn legacy_ingest_cannot_launder_provider_gap_or_reconciliation_receipt() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:windows-uia:mta:legacy-isolation");
    let target = TargetIncarnationRef::from("target:windows:selection=legacy-isolation");

    bridge
        .bind_provider_observation(binding(
            session_id,
            provider.clone(),
            target.clone(),
            EventContinuityState::OrderingOpaque,
            Some(0),
        ))
        .await
        .unwrap();
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(3)],
        })
        .await;
    assert!(
        bridge
            .record_reconciliation(session_id, receipt(provider, target))
            .await
    );

    let before = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(before.event_continuity, EventContinuityState::GapDetected);
    assert!(before.gap.is_some());
    assert_eq!(
        before.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );

    let _ = bridge
        .ingest(ObserverBatch {
            session_id,
            generation: 0,
            events: vec![event(1)],
        })
        .await;

    let after = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(after.event_continuity, EventContinuityState::GapDetected);
    assert_eq!(after.gap, before.gap);
    assert_eq!(
        after.current_snapshot_completeness,
        Some(ReconciliationCompleteness::Established)
    );
    assert_eq!(
        after.reconciliation_receipt_id.as_deref(),
        Some("reconcile:uia:1")
    );
}

#[tokio::test]
async fn rebinding_an_existing_session_cannot_launder_adverse_continuity() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:windows-uia:mta:3");
    let target = TargetIncarnationRef::from("target:windows:selection=3");

    bridge
        .bind_provider_observation(binding(
            session_id,
            provider.clone(),
            target.clone(),
            EventContinuityState::OrderingOpaque,
            Some(0),
        ))
        .await
        .unwrap();
    bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 1,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(1), event(3)],
        })
        .await;

    let error = bridge
        .bind_provider_observation(binding(
            session_id,
            provider,
            target,
            EventContinuityState::Continuous,
            Some(0),
        ))
        .await
        .unwrap_err();
    assert_eq!(error, ProviderObservationBindingError::AlreadyBound);
    assert_eq!(
        bridge
            .observation_status(session_id)
            .await
            .unwrap()
            .event_continuity,
        EventContinuityState::GapDetected
    );
}

#[tokio::test]
async fn evidence_backed_states_cannot_be_claimed_by_initial_declaration() {
    let bridge = LiveBridge::new(32, 8);
    let error = bridge
        .bind_provider_observation(binding(
            Uuid::new_v4(),
            ProviderIncarnationRef::from("provider:windows-uia:mta:4"),
            TargetIncarnationRef::from("target:windows:selection=4"),
            EventContinuityState::GapDetected,
            Some(0),
        ))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ProviderObservationBindingError::UnsupportedInitialContinuity
    );
}
