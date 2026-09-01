use chrono::Utc;
use localview_live_bridge::{LiveBridge, ObserverEvent, ObserverEventKind, ProviderObserverBatch};
use localview_protocol::{EventContinuityState, ProviderIncarnationRef, TargetIncarnationRef};
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

#[tokio::test]
async fn sequence_reset_rebases_the_bounded_lineage_without_claiming_continuity() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let provider = ProviderIncarnationRef::from("provider:webview:stable");
    let target = TargetIncarnationRef::from("target:webview:stable");

    let first = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 7,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            events: vec![event(10), event(11)],
        })
        .await;
    assert_eq!(first.ingest.accepted, 2);

    let reset = bridge
        .ingest_provider(ProviderObserverBatch {
            session_id,
            generation: 7,
            provider_incarnation_ref: provider,
            target_incarnation_ref: target,
            events: vec![event(1), event(2)],
        })
        .await;

    assert_eq!(reset.continuity, EventContinuityState::SequenceReset);
    assert_eq!(reset.ingest.accepted, 2);
    assert_eq!(reset.ingest.rejected_stale, 0);
    assert_eq!(reset.ingest.last_seq, Some(2));

    let recent = bridge.recent(session_id, 8).await;
    assert_eq!(recent.iter().map(|item| item.seq).collect::<Vec<_>>(), vec![1, 2]);

    let status = bridge.observation_status(session_id).await.unwrap();
    assert_eq!(status.event_continuity, EventContinuityState::SequenceReset);
    assert_eq!(status.last_seq, Some(2));
}
