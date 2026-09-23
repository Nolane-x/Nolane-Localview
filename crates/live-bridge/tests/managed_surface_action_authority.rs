use chrono::Utc;
use localview_live_bridge::{
    BridgeActionKind, BridgeActionResult, LiveBridge, ManagedSurfaceActionCompletion,
    ProviderObservationBinding,
};
use localview_protocol::{
    EventContinuityState, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
};
use serde_json::Value;
use uuid::Uuid;

fn session() -> SessionId {
    Uuid::new_v4()
}

#[tokio::test]
async fn authority_transition_retires_queued_and_inflight_public_actions() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = session();

    let stale_before_authority = bridge
        .enqueue_action(session_id, None, BridgeActionKind::Focus)
        .await;
    assert!(
        bridge
            .take_managed_surface_actions(session_id, "surface:A".into(), 8)
            .await
            .is_empty(),
        "the first managed authority must fail closed over pre-authority work"
    );
    assert!(
        bridge
            .request_action_cancellation(session_id, stale_before_authority.id)
            .await
            .is_none(),
        "retired queued work must leave cancellation authority"
    );

    let inflight_a = bridge
        .enqueue_action(session_id, None, BridgeActionKind::Snapshot)
        .await;
    let taken_a = bridge
        .take_managed_surface_actions(session_id, "surface:A".into(), 8)
        .await;
    assert_eq!(taken_a.len(), 1);
    assert_eq!(taken_a[0].id, inflight_a.id);

    let queued_a = bridge
        .enqueue_action(session_id, None, BridgeActionKind::Measure)
        .await;
    assert!(
        bridge
            .take_managed_surface_actions(session_id, "surface:B".into(), 8)
            .await
            .is_empty(),
        "a new primary surface must never inherit queued work from the old executor"
    );
    assert!(
        bridge
            .request_action_cancellation(session_id, queued_a.id)
            .await
            .is_none(),
        "retired queued work must be fully removed"
    );

    let stale_completion = BridgeActionResult {
        action_id: inflight_a.id,
        ok: true,
        error: None,
        payload: Value::Null,
        completed_at: Utc::now(),
    };
    assert_eq!(
        bridge
            .complete_managed_surface_action(session_id, "surface:A", stale_completion.clone())
            .await,
        ManagedSurfaceActionCompletion::AuthorityStale
    );
    assert_eq!(
        bridge
            .complete_managed_surface_action(session_id, "surface:B", stale_completion)
            .await,
        ManagedSurfaceActionCompletion::ActionNotInflight,
        "the new authority must not be able to complete an action dispatched to the old surface"
    );

    let fresh_b = bridge
        .enqueue_action(session_id, None, BridgeActionKind::Snapshot)
        .await;
    let taken_b = bridge
        .take_managed_surface_actions(session_id, "surface:B".into(), 8)
        .await;
    assert_eq!(taken_b.len(), 1);
    assert_eq!(taken_b[0].id, fresh_b.id);
    assert_eq!(
        bridge
            .complete_managed_surface_action(
                session_id,
                "surface:B",
                BridgeActionResult {
                    action_id: fresh_b.id,
                    ok: true,
                    error: None,
                    payload: Value::Null,
                    completed_at: Utc::now(),
                },
            )
            .await,
        ManagedSurfaceActionCompletion::Completed
    );
}

#[tokio::test]
async fn managed_surface_authority_is_independent_from_provider_observation() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = session();
    let provider = ProviderIncarnationRef::from("provider:native:independent");
    let target = TargetIncarnationRef::from("target:native:independent");

    bridge
        .bind_provider_observation(ProviderObservationBinding {
            session_id,
            generation: 9,
            provider_incarnation_ref: provider.clone(),
            target_incarnation_ref: target.clone(),
            initial_continuity: EventContinuityState::Continuous,
            sequence_baseline: Some(17),
        })
        .await
        .expect("bind native observation");

    assert!(
        bridge
            .take_managed_surface_actions(session_id, "surface:preview".into(), 8)
            .await
            .is_empty()
    );

    let status = bridge
        .observation_status(session_id)
        .await
        .expect("provider observation remains bound");
    assert_eq!(status.provider_incarnation_ref, provider);
    assert_eq!(status.target_incarnation_ref, target);
    assert_eq!(status.event_continuity, EventContinuityState::Continuous);
    assert_eq!(status.last_seq, Some(17));
}
