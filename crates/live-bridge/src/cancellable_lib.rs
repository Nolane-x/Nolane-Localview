#![forbid(unsafe_code)]

#[path = "lib.rs"]
mod base;

pub use base::{
    BridgeAction, BridgeActionKind, BridgeActionResult, CompletionOrigin, IngestReport,
    NativeExecutorAction, NativeExecutorRequest, NativeExecutorResult, ObserverBatch, ObserverEvent,
    ObserverEventKind, PrivateBridgeAction, PrivateCaptureActionData,
};

use std::{
    collections::HashMap,
    ops::Deref,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

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
    Completed,
}

#[derive(Debug, Clone)]
struct NativeExecutorLifecycleEntry {
    state: NativeExecutorLifecycle,
    cancellation_requested_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct CancellationAuthority {
    requests: HashMap<(SessionId, Uuid), NativeExecutorLifecycleEntry>,
}

#[derive(Clone, Debug)]
pub struct LiveBridge {
    base: base::LiveBridge,
    cancellation: Arc<RwLock<CancellationAuthority>>,
}

impl LiveBridge {
    pub fn new(event_capacity: usize, action_capacity: usize) -> Self {
        Self {
            base: base::LiveBridge::new(event_capacity, action_capacity),
            cancellation: Arc::new(RwLock::new(CancellationAuthority::default())),
        }
    }

    pub async fn enqueue_native_executor(
        &self,
        session_id: SessionId,
        action: NativeExecutorAction,
    ) -> NativeExecutorRequest {
        let request = self.base.enqueue_native_executor(session_id, action).await;
        self.cancellation.write().await.requests.insert(
            (session_id, request.id),
            NativeExecutorLifecycleEntry {
                state: NativeExecutorLifecycle::Pending,
                cancellation_requested_at: None,
            },
        );
        request
    }

    pub async fn take_native_executor_requests(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorRequest> {
        let taken = self.base.take_native_executor_requests(session_id, limit).await;
        let mut deliver = Vec::with_capacity(taken.len());
        let mut cancelled = Vec::new();

        {
            let mut authority = self.cancellation.write().await;
            for request in taken {
                let key = (session_id, request.id);
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
                                cancellation_requested_at: None,
                            },
                        );
                        deliver.push(request);
                    }
                }
            }
        }

        for request in cancelled {
            let _ = self
                .settle_cancelled_origin(session_id, request.id, "cancelled before native dispatch")
                .await;
        }

        deliver
    }

    pub async fn claim_native_executor(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorRequest> {
        let terminal_cancelled = self
            .cancellation
            .read()
            .await
            .requests
            .get(&(session_id, request_id))
            .is_some_and(|entry| entry.state == NativeExecutorLifecycle::Cancelled);
        if terminal_cancelled {
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
        let terminal_cancelled = self
            .cancellation
            .read()
            .await
            .requests
            .get(&(session_id, request_id))
            .is_some_and(|entry| entry.state == NativeExecutorLifecycle::Cancelled);
        if terminal_cancelled {
            return false;
        }

        let completed = self.base.complete_native_executor(session_id, result).await;
        if completed {
            if let Some(entry) = self
                .cancellation
                .write()
                .await
                .requests
                .get_mut(&(session_id, request_id))
            {
                entry.state = NativeExecutorLifecycle::Completed;
                entry.cancellation_requested_at = None;
            }
        }
        completed
    }

    pub async fn request_native_executor_cancellation(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> Option<NativeExecutorCancellationOutcome> {
        let mut authority = self.cancellation.write().await;
        let entry = authority.requests.get_mut(&(session_id, request_id))?;
        let outcome = match entry.state {
            NativeExecutorLifecycle::Pending => {
                entry.state = NativeExecutorLifecycle::Cancelled;
                entry.cancellation_requested_at = Some(Utc::now());
                NativeExecutorCancellationOutcome {
                    request_id,
                    state: NativeExecutorCancellationState::Cancelled,
                    acknowledged: true,
                }
            }
            NativeExecutorLifecycle::Inflight => {
                entry.state = NativeExecutorLifecycle::CancellationRequested;
                entry.cancellation_requested_at = Some(Utc::now());
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
            NativeExecutorLifecycle::Completed => return None,
        };
        Some(outcome)
    }

    pub async fn native_executor_cancellations(
        &self,
        session_id: SessionId,
        limit: usize,
    ) -> Vec<NativeExecutorCancellationSignal> {
        let authority = self.cancellation.read().await;
        authority
            .requests
            .iter()
            .filter_map(|((owner, request_id), entry)| {
                if *owner != session_id || entry.state != NativeExecutorLifecycle::CancellationRequested {
                    return None;
                }
                Some(NativeExecutorCancellationSignal {
                    request_id: *request_id,
                    requested_at: entry.cancellation_requested_at?,
                })
            })
            .take(limit)
            .collect()
    }

    pub async fn acknowledge_native_executor_cancellation(
        &self,
        session_id: SessionId,
        request_id: Uuid,
    ) -> bool {
        let current = self
            .cancellation
            .read()
            .await
            .requests
            .get(&(session_id, request_id))
            .map(|entry| entry.state);
        match current {
            Some(NativeExecutorLifecycle::Cancelled) => return true,
            Some(NativeExecutorLifecycle::CancellationRequested) => {}
            _ => return false,
        }

        if !self
            .settle_cancelled_origin(session_id, request_id, "native executor cancellation acknowledged")
            .await
        {
            return false;
        }

        if let Some(entry) = self
            .cancellation
            .write()
            .await
            .requests
            .get_mut(&(session_id, request_id))
        {
            entry.state = NativeExecutorLifecycle::Cancelled;
            return true;
        }
        false
    }

    pub async fn release_session(&self, session_id: SessionId) {
        self.base.release_session(session_id).await;
        self.cancellation
            .write()
            .await
            .requests
            .retain(|(owner, _), _| *owner != session_id);
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
