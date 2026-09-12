use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use chrono::Utc;
use localview_control::{wait_for_native_executor_result_with_timeout, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{
    LiveBridge, NativeExecutorAction, NativeExecutorCancellationState, NativeExecutorResult,
};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind, ViewportMeta,
};
use localview_sessions::SessionManager;
use serde_json::Value;

fn discovered(port: u16) -> DiscoveredServer {
    DiscoveredServer {
        candidate: ListenerCandidate {
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port,
                scheme: "http".into(),
            },
            pid: Some(u32::from(port)),
            process_name: Some("node".into()),
            command: Some("vite".into()),
            cwd: Some(format!("/tmp/localview-timeout-cancel-{port}")),
        },
        classification: Classification {
            kind: ServerKind::FrontendDevServer,
            confidence: 1.0,
            framework: Some("Vite".into()),
            title: Some(format!("Timeout Cancel {port}")),
            hmr_detected: true,
            evidence: Default::default(),
        },
    }
}

async fn test_state() -> (ControlState, localview_protocol::SessionId) {
    let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
    let reconcile = sessions
        .reconcile(vec![discovered(5373)], Utc::now())
        .await;
    let session_id = reconcile.created[0];
    let state = ControlState {
        token: Arc::from("test-token"),
        sessions,
        observations: ObservationBus::new(32),
        live: LiveBridge::default(),
        evidence: EvidenceStore::new(128),
        paused: Arc::new(AtomicBool::new(false)),
    };
    (state, session_id)
}

fn action() -> NativeExecutorAction {
    NativeExecutorAction::VisualDiffCapture {
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 720,
            device_scale_factor: 1.0,
        },
        revision: Some("timeout-cancel".into()),
    }
}

fn result(request_id: uuid::Uuid, marker: &str) -> NativeExecutorResult {
    NativeExecutorResult {
        request_id,
        ok: true,
        error: None,
        usage: None,
        payload: serde_json::json!({"marker": marker}),
        completed_at: Utc::now(),
    }
}

async fn complete_one(
    state: &ControlState,
    session_id: localview_protocol::SessionId,
    marker: &str,
) -> uuid::Uuid {
    let request = state.live.enqueue_native_executor(session_id, action()).await;
    let dispatched = state
        .live
        .take_native_executor_requests(session_id, 1)
        .await;
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].id, request.id);
    assert!(
        state
            .live
            .claim_native_executor(session_id, request.id)
            .await
            .is_some()
    );
    assert!(
        state
            .live
            .complete_native_executor(session_id, result(request.id, marker))
            .await
    );
    request.id
}

#[test]
fn native_visual_consumers_share_exact_cancellable_waiter_authority() {
    let visual_verify = include_str!("../src/visual_verify.rs");
    let perception_cycle = include_str!("../src/perception_cycle.rs");

    assert!(visual_verify.contains("wait_for_native_executor_result_with_timeout"));
    assert!(perception_cycle.contains("wait_for_native_executor_result_with_timeout"));
    assert!(!visual_verify.contains("recent_native_executor_results(id, 16)"));
    assert!(!perception_cycle.contains("recent_native_executor_results(id, 16)"));
}

#[tokio::test]
async fn queued_native_visual_timeout_cancels_before_dispatch() {
    let (state, session_id) = test_state().await;
    let request = state.live.enqueue_native_executor(session_id, action()).await;

    assert!(
        wait_for_native_executor_result_with_timeout(
            &state,
            session_id,
            request.id,
            Duration::from_millis(20),
        )
        .await
        .is_err(),
        "missing native result must time out"
    );

    let repeated = state
        .live
        .request_native_executor_cancellation(session_id, request.id)
        .await
        .expect("timeout leaves a bounded idempotent cancellation tombstone");
    assert_eq!(repeated.state, NativeExecutorCancellationState::Cancelled);
    assert!(repeated.acknowledged);
    assert!(
        state
            .live
            .take_native_executor_requests(session_id, 8)
            .await
            .is_empty(),
        "timed-out queued work must never dispatch to the native worker"
    );
}

#[tokio::test]
async fn inflight_native_visual_timeout_fences_result_before_worker_ack() {
    let (state, session_id) = test_state().await;
    let request = state.live.enqueue_native_executor(session_id, action()).await;
    let dispatched = state
        .live
        .take_native_executor_requests(session_id, 8)
        .await;
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].id, request.id);

    assert!(
        wait_for_native_executor_result_with_timeout(
            &state,
            session_id,
            request.id,
            Duration::from_millis(20),
        )
        .await
        .is_err(),
        "missing inflight native result must time out"
    );

    let signal = state
        .live
        .native_executor_cancellation(session_id, request.id)
        .await
        .expect("timeout must request cooperative cancellation for inflight work");
    assert_eq!(signal.request_id, request.id);
    assert!(
        state
            .live
            .claim_native_executor(session_id, request.id)
            .await
            .is_none(),
        "accepted timeout cancellation must fence result ownership before ACK"
    );
}

#[tokio::test]
async fn waiter_finds_exact_native_result_outside_recent_window() {
    let (state, session_id) = test_state().await;
    let target_id = complete_one(&state, session_id, "target").await;

    for index in 0..20 {
        complete_one(&state, session_id, &format!("newer-{index}")).await;
    }

    let recent = state.live.recent_native_executor_results(session_id, 16).await;
    assert_eq!(recent.len(), 16);
    assert!(
        recent.iter().all(|item| item.request_id != target_id),
        "test precondition: target must be outside the legacy recent-16 window"
    );

    let resolved = wait_for_native_executor_result_with_timeout(
        &state,
        session_id,
        target_id,
        Duration::from_millis(30),
    )
    .await
    .expect("exact retained result must resolve even when newer completions hide it from recent-16");

    assert_eq!(resolved.request_id, target_id);
    assert_eq!(
        resolved.payload.get("marker"),
        Some(&Value::String("target".into()))
    );
}
