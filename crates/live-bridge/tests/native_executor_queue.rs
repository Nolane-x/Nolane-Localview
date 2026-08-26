use chrono::Utc;
use localview_live_bridge::{LiveBridge, NativeExecutorAction, NativeExecutorResult};
use localview_protocol::ViewportMeta;
use localview_token_budget::{
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetUsage,
};
use uuid::Uuid;

fn budget() -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: 800,
        text_tokens: 400,
        image_regions: 1,
        chromium_spawns: 0,
    }
}

fn visual_action() -> NativeExecutorAction {
    NativeExecutorAction::VisualPacket {
        reference: Some("@save".into()),
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.25,
        },
        revision: Some("rev-1".into()),
        budget: budget(),
        budget_escalation_reason: Some(BudgetEscalationReason::InsufficientEvidence),
    }
}

fn result(request_id: Uuid) -> NativeExecutorResult {
    NativeExecutorResult {
        request_id,
        ok: true,
        error: None,
        usage: Some(PerceptionBudgetUsage {
            latency_ms: 123,
            text_tokens: 77,
            image_regions: 1,
            chromium_spawns: 0,
        }),
        payload: serde_json::json!({"receipt": "bounded-metadata-only"}),
        completed_at: Utc::now(),
    }
}

#[tokio::test]
async fn native_executor_requests_are_isolated_from_page_actions() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();

    let request = bridge
        .enqueue_native_executor(session_id, visual_action())
        .await;

    assert!(bridge.take_actions(session_id, 8).await.is_empty());
    let native = bridge.take_native_executor_requests(session_id, 8).await;
    assert_eq!(native.len(), 1);
    assert_eq!(native[0].id, request.id);
}

#[tokio::test]
async fn result_requires_exact_claimed_native_origin_and_session() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let other_session = Uuid::new_v4();
    let request = bridge
        .enqueue_native_executor(session_id, visual_action())
        .await;

    assert!(bridge
        .claim_native_executor(session_id, request.id)
        .await
        .is_none());

    let taken = bridge.take_native_executor_requests(session_id, 1).await;
    assert_eq!(taken.len(), 1);
    assert!(bridge
        .claim_native_executor(other_session, request.id)
        .await
        .is_none());

    let claimed = bridge
        .claim_native_executor(session_id, request.id)
        .await
        .expect("exact native request becomes claimable only after take");
    assert_eq!(claimed.id, request.id);

    assert!(bridge
        .complete_native_executor(session_id, result(request.id))
        .await);
    let results = bridge.recent_native_executor_results(session_id, 8).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].request_id, request.id);
}

#[tokio::test]
async fn native_executor_queue_is_bounded_by_action_capacity() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();

    for _ in 0..20 {
        bridge
            .enqueue_native_executor(session_id, visual_action())
            .await;
    }

    let taken = bridge.take_native_executor_requests(session_id, 64).await;
    assert_eq!(taken.len(), 8);
}

#[tokio::test]
async fn active_native_origins_are_never_evicted_by_later_polls() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();

    let mut first_ids = Vec::new();
    for _ in 0..8 {
        first_ids.push(
            bridge
                .enqueue_native_executor(session_id, visual_action())
                .await
                .id,
        );
    }
    let first_taken = bridge.take_native_executor_requests(session_id, 8).await;
    assert_eq!(first_taken.len(), 8);

    for _ in 0..8 {
        bridge
            .enqueue_native_executor(session_id, visual_action())
            .await;
    }

    assert!(
        bridge
            .take_native_executor_requests(session_id, 8)
            .await
            .is_empty(),
        "pending work must wait instead of evicting active inflight origins"
    );

    let first = first_ids[0];
    assert!(bridge.claim_native_executor(session_id, first).await.is_some());
    assert!(bridge
        .complete_native_executor(session_id, result(first))
        .await);

    let newly_available = bridge.take_native_executor_requests(session_id, 8).await;
    assert_eq!(
        newly_available.len(),
        1,
        "exactly one pending request may enter inflight after one active origin completes"
    );

    let still_active = first_ids[1];
    assert!(
        bridge
            .claim_native_executor(session_id, still_active)
            .await
            .is_some(),
        "older inflight origin must remain claimable after later polls"
    );
    assert!(bridge
        .complete_native_executor(session_id, result(still_active))
        .await);
}

#[tokio::test]
async fn expired_native_origins_release_capacity_and_reject_late_results() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let mut active_ids = Vec::new();
    for _ in 0..8 {
        active_ids.push(
            bridge
                .enqueue_native_executor(session_id, visual_action())
                .await
                .id,
        );
    }
    assert_eq!(bridge.take_native_executor_requests(session_id, 8).await.len(), 8);
    let claimed_id = active_ids[0];
    assert!(bridge
        .claim_native_executor(session_id, claimed_id)
        .await
        .is_some());

    for _ in 0..8 {
        bridge
            .enqueue_native_executor(session_id, visual_action())
            .await;
    }

    let expired = bridge
        .expire_native_executor_active_before(
            session_id,
            Utc::now() + chrono::Duration::seconds(1),
        )
        .await;
    assert_eq!(expired, 8, "all active inflight/claimed origins must expire");
    assert!(
        !bridge
            .complete_native_executor(session_id, result(claimed_id))
            .await,
        "a late result must not resurrect an expired authority origin"
    );
    assert_eq!(
        bridge.take_native_executor_requests(session_id, 8).await.len(),
        8,
        "pending work must regain all active capacity after stale origins expire"
    );
}

#[tokio::test]
async fn fresh_native_origins_survive_lease_cleanup() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let request = bridge
        .enqueue_native_executor(session_id, visual_action())
        .await;
    assert_eq!(bridge.take_native_executor_requests(session_id, 1).await.len(), 1);

    let expired = bridge
        .expire_native_executor_active_before(
            session_id,
            Utc::now() - chrono::Duration::seconds(1),
        )
        .await;
    assert_eq!(expired, 0);
    assert!(bridge
        .claim_native_executor(session_id, request.id)
        .await
        .is_some());
}

#[tokio::test]
async fn native_executor_result_error_truncation_is_utf8_safe() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let request = bridge
        .enqueue_native_executor(session_id, visual_action())
        .await;
    assert_eq!(bridge.take_native_executor_requests(session_id, 1).await.len(), 1);
    assert!(bridge
        .claim_native_executor(session_id, request.id)
        .await
        .is_some());

    let mut unicode_result = result(request.id);
    unicode_result.ok = false;
    unicode_result.error = Some("€".repeat(800));
    unicode_result.usage = None;

    assert!(bridge
        .complete_native_executor(session_id, unicode_result)
        .await);
    let stored = bridge.recent_native_executor_results(session_id, 1).await;
    let error = stored[0].error.as_deref().expect("bounded error is retained");
    assert!(error.len() <= 2 * 1024);
    assert!(error.chars().all(|character| character == '€'));
}
