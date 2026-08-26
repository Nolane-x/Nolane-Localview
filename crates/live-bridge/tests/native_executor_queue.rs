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

    bridge
        .complete_native_executor(session_id, result(request.id))
        .await;
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
