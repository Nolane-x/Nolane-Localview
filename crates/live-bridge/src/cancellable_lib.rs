#![forbid(unsafe_code)]

#[path = "lib.rs"]
mod base;

pub use base::{
    BridgeAction, BridgeActionKind, BridgeActionResult, CompletionOrigin, IngestReport,
    NativeExecutorAction, NativeExecutorRequest, NativeExecutorResult, ObserverBatch, ObserverEvent,
    ObserverEventKind, PrivateBridgeAction, PrivateCaptureActionData,
};

use std::{
    collections::{HashMap, VecDeque},
    ops::Deref,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

const MIN_NATIVE_EXECUTOR_CAPACITY: usize = 8;
const MAX_CANCELLED_TOMBSTONES: usize = 256;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeExecutorCancellationState {
    CancellationRequested,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeExecutorCancellationSignal {
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeExecutorCancellationOutcome {
    pub request_id: Uuid,
    pub state: NativeExecutorCancellationState,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeExecutorLifecycle {
    Pending,
    Inflight,
    CancellationRequested,
    Cancelled,
}

#[derive(Debug, Clone)]
struct NativeExecutorLifecycleEntry {
    state: NativeExecutorLifecycle,
    created_at: DateTime<Utc>,
    cancellation_requested_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct CancellationAuthority {
    requests: HashMap<(SessionId, Uuid), NativeExecutorLifecycleEntry>,
    queued: HashMap<SessionId, VecDeque<Uuid>>,
    cancelled_order: VecDeque<(SessionId, Uuid)>,
}

impl CancellationAuthority {
    fn record_enqueued(&mut self, session_id: SessionId, request: &NativeExecutorRequest, capacity: usize) {
        let key = (session_id, request.id);
        self.requests.insert(
            key,
            NativeExecutorLifecycleEntry {
                state: NativeExecutorLifecycle::Pending,
                created_at: request.created_at,
                cancellation_requested_at: None,
            },
        );

        let queue = self.queued.entry(session_id).or_default();
        queue.push_back(request.id);
        while queue.len() > capacity {
            let Some(evicted_id) = queue.pop_front() else {
                break;
            };
            let evicted_key = (session_id, evicted_id);
            if self
                .requests
                .get(&evicted_key)
                .is_some_and(|entry| entry.state == NativeExecutorLifecycle::Pending)
            {
                self.requests.remove(&evicted_key);
            }
        }
        self.prune_cancelled_tombstones();
    }

    fn remove_queued(&mut self, session_id: SessionId, request_id: Uuid) {
        let mut remove_session_queue = false;
        if let Some(queue) = self.queued.get_mut(&session_id) {
            if let Some(index) = queue.iter().position(|id| *id == request_id) {
                queue.remove(index);
            }
            remove_session_queue = queue.is_empty();
        }
        if remove_session_queue {
            self.queued.remove(&session_id);
        }
    }

    fn is_queued(&self, key: (SessionId, Uuid)) -> bool {
        self.queued
            .get(&key.0)
            .is_some_and(|queue| queue.iter().any(|id| *id == key.1))
    }

    fn mark_cancelled(&mut self, key: (SessionId, Uuid), requested_at: DateTime<Utc>) {
        let already_cancelled = self
            .requests
            .get(&key)
            .is_some_and(|entry| entry.state == NativeExecutorLifecycle::Cancelled);
        if let Some(entry) = self.requests.get_mut(&key) {
            entry.state = NativeExecutorLifecycle::Cancelled;
            entry.cancellation_requested_at = Some(requested_at);
        }
        if !already_cancelled {
            self.cancelled_order.push_back(key);
        }
        self.prune_cancelled_tombstones();
    }

    fn prune_cancelled_tombstones(&mut self) {
        while self.cancelled_order.len() > MAX_CANCELLED_TOMBSTONES {
            let Some(index) = self
                .cancelled_order
                .iter()
                .position(|key| !self.is_queued(*key))
            else {
                break;
            };
            let Some(key) = self.cancelled_order.remove(index) else {
                break;
            };
            if self
                .requests
                .get(&key)
                .is_some_and(|entry| entry.state == NativeExecutorLifecycle::Cancelled)
            {
                self.requests.remove(&key);
            }
        }
    }

    fn remove_request(&mut self, key: (SessionId, Uuid)) {
        self.requests.remove(&key);
        self.remove_queued(key.0, key.1);
    }

    fn release_session(&mut self, session_id: SessionId) {
        self.requests.retain(|(owner, _), _| *owner != session_id);
        self.queued.remove(&session_id);
        self.cancelled_order.retain(|(owner, _)| *owner != session_id);
    }
}

#[derive(Clone, Debug)]
pub struct LiveBridge {
    base: base::LiveBridge,
    cancellation: Arc<Mutex<CancellationAuthority>>,
    action_capacity: usize,
}

impl LiveBridge {
    pub fn new(event_capacity: usize, action_capacity: usize) -> Self {
        let action_capacity = action_capacity.max(MIN_NATIVE_EXECUTOR_CAPACITY);
        Self {
            base: base::LiveBridge::new(event_capacity, action_capacity),
            cancellation: Arc::new(Mutex::new(CancellationAuthority::default())),
            action_capacity,
        }
    }

    pub async fn enqueue_native_executor(
        &self,
        session_id: SessionId,
        action: NativeExecutorAction,
    ) -> NativeExecutorRequest {
        let mut authority = self.cancellation.lock().await;
        let request = self.base.enqueue_native_executor(session_id, action).await;
        authority.record_enqueued(session_id, &request, self.action_capacity);
        request
    }

    pub async fn take_native_executor_requests(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorRequest> {
        let mut authority = self.cancellation.lock().await;
        let taken = self.base.take_native_executor_requests(session_id, limit).await;
        let mut deliver = Vec::with_capacity(taken.len());
        let mut cancelled = Vec::new();

        for request in taken {
            let key = (session_id, request.id);
            authority.remove_queued(session_id, request.id);
            match authority.requests.get_mut(&key) {
                Some(entry) if entry.state == NativeExecutorLifecycle::Cancelled => {
                    cancelled.push(request);
                }
                Some(entry) => {
                    entry.state = NativeExecutorLifecycle::Inflight;
                    deliver.push(request);
                }
                None => {
                    authority.requests.insert(
                        key,
                        NativeExecutorLifecycleEntry {
                            state: NativeExecutorLifecycle::Inflight,
                            created_at: request.created_at,
                            cancellation_requested_at: None,
                        },
                    );
                    deliver.push(request);
                }
            }
        }

        for request in cancelled {
            let _ = self
                .settle_cancelled_origin(
                    session_id,
                    request.id,
                    "cancelled before native dispatch",
                )
                .await;
        }
        authority.prune_cancelled_tombstones();
        deliver
    }

    pub async fn claim_native_executor(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorRequest> {
        let authority = self.cancellation.lock().await;
        let state = authority
            .requests
            .get(&(session_id, request_id))
            .map(|entry| entry.state)?;
        if matches!(
            state,
            NativeExecutorLifecycle::CancellationRequested | NativeExecutorLifecycle::Cancelled
        ) {
            return None;
        }
        self.base
            .claim_native_executor(session_id, request_id)
            .await
    }

    pub async fn complete_native_executor(
        &self,
        session_id: SessionId,
        result: NativeExecutorResult,
    ) -> bool {
        let request_id = result.request_id;
        let key = (session_id, request_id);
        let mut authority = self.cancellation.lock().await;
        let Some(state) = authority.requests.get(&key).map(|entry| entry.state) else {
            return false;
        };
        if matches!(
            state,
            NativeExecutorLifecycle::CancellationRequested | NativeExecutorLifecycle::Cancelled
        ) {
            return false;
        }

        let completed = self.base.complete_native_executor(session_id, result).await;
        if completed {
            authority.remove_request(key);
        }
        completed
    }

    pub async fn native_executor_result(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorResult> {
        self.base.native_executor_result(session_id, request_id).await
    }

    pub async fn request_native_executor_cancellation(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorCancellationOutcome> {
        let key = (session_id, request_id);
        let mut authority = self.cancellation.lock().await;
        let state = authority.requests.get(&key)?.state;
        let outcome = match state {
            NativeExecutorLifecycle::Pending => {
                let requested_at = Utc::now();
                authority.mark_cancelled(key, requested_at);
                NativeExecutorCancellationOutcome {
                    request_id,
                    state: NativeExecutorCancellationState::Cancelled,
                    acknowledged: true,
                }
            }
            NativeExecutorLifecycle::Inflight => {
                let requested_at = Utc::now();
                if let Some(entry) = authority.requests.get_mut(&key) {
                    entry.state = NativeExecutorLifecycle::CancellationRequested;
                    entry.cancellation_requested_at = Some(requested_at);
                }
                NativeExecutorCancellationOutcome {
                    request_id,
                    state: NativeExecutorCancellationState::CancellationRequested,
                    acknowledged: false,
                }
            }
            NativeExecutorLifecycle::CancellationRequested => NativeExecutorCancellationOutcome {
                request_id,
                state: NativeExecutorCancellationState::CancellationRequested,
                acknowledged: false,
            },
            NativeExecutorLifecycle::Cancelled => NativeExecutorCancellationOutcome {
                request_id,
                state: NativeExecutorCancellationState::Cancelled,
                acknowledged: true,
            },
        };
        Some(outcome)
    }

    pub async fn native_executor_cancellation(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorCancellationSignal> {
        let authority = self.cancellation.lock().await;
        let entry = authority.requests.get(&(session_id, request_id))?;
        if entry.state != NativeExecutorLifecycle::CancellationRequested {
            return None;
        }
        Some(NativeExecutorCancellationSignal {
            request_id,
            requested_at: entry.cancellation_requested_at?,
        })
    }

    pub async fn native_executor_cancellations(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorCancellationSignal> {
        let authority = self.cancellation.lock().await;
        let mut signals = authority
            .requests
            .iter()
            .filter_map(|((owner, request_id), entry)| {
                if *owner != session_id
                    || entry.state != NativeExecutorLifecycle::CancellationRequested
                {
                    return None;
                }
                Some(NativeExecutorCancellationSignal {
                    request_id: *request_id,
                    requested_at: entry.cancellation_requested_at?,
                })
            })
            .collect::<Vec<_>>();
        signals.sort_by_key(|signal| signal.requested_at);
        signals.into_iter().take(limit).collect()
    }

    pub async fn acknowledge_native_executor_cancellation(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> bool {
        let key = (session_id, request_id);
        let mut authority = self.cancellation.lock().await;
        let Some(state) = authority.requests.get(&key).map(|entry| entry.state) else {
            return false;
        };
        match state {
            NativeExecutorLifecycle::Cancelled => return true,
            NativeExecutorLifecycle::CancellationRequested => {}
            _ => return false,
        }

        if !self
            .settle_cancelled_origin(
                session_id,
                request_id,
                "native executor cancellation acknowledged",
            )
            .await
        {
            return false;
        }

        let requested_at = authority
            .requests
            .get(&key)
            .and_then(|entry| entry.cancellation_requested_at)
            .unwrap_or_else(Utc::now);
        authority.mark_cancelled(key, requested_at);
        true
    }

    pub async fn expire_native_executor_active_before(
        &self,
        session_id: SessionId,
        cutoff: DateTime<Utc>,
    ) -> usize {
        let mut authority = self.cancellation.lock().await;
        let expired = self
            .base
            .expire_native_executor_active_before(session_id, cutoff)
            .await;
        if expired > 0 {
            authority.requests.retain(|(owner, _), entry| {
                !(*owner == session_id
                    && matches!(
                        entry.state,
                        NativeExecutorLifecycle::Inflight
                            | NativeExecutorLifecycle::CancellationRequested
                    )
                    && entry.created_at < cutoff)
            });
        }
        expired
    }

    pub async fn release_session(&self, session_id: SessionId) {
        let mut authority = self.cancellation.lock().await;
        self.base.release_session(session_id).await;
        authority.release_session(session_id);
    }

    async fn settle_cancelled_origin(
        &self,
        session_id: SessionId,
        request_id: Uuid,
        reason: &str,
    ) -> bool {
        let _ = self
            .base
            .claim_native_executor(session_id, request_id)
            .await;
        self.base
            .complete_native_executor(
                session_id,
                NativeExecutorResult {
                    request_id,
                    ok: false,
                    error: Some(reason.into()),
                    usage: None,
                    payload: Value::Null,
                    completed_at: Utc::now(),
                },
            )
            .await
    }
}

impl Deref for LiveBridge {
    type Target = base::LiveBridge;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl Default for LiveBridge {
    fn default() -> Self {
        Self::new(2048, 128)
    }
}
