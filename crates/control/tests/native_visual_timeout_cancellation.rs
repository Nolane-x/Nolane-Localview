use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use chrono::Utc;
use localview_control::{wait_for_native_visual_diff_with_timeout, ControlState};
use localview_evidence::EvidenceStore;
use localview_live_bridge::{
    LiveBridge, NativeExecutorAction, NativeExecutorCancellationState,
};
use localview_observation::ObservationBus;
use localview_protocol::{
    Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind, ViewportMeta,
};
use localview_sessions::SessionManager;

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

#[tokio::test]
async fn queued_native_visual_timeout_cancels_before_dispatch() {
    let (state, session_id) = test_state().await;
    let request = state.live.enqueue_native_executor(session_id, action()).await;

    assert!(
        wait_for_native_visual_diff_with_timeout(
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
        wait_for_native_visual_diff_with_timeout(
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
