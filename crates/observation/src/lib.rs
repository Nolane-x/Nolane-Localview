#![forbid(unsafe_code)]

use std::{collections::VecDeque, sync::Arc};
use localview_protocol::ObservationEvent;
use tokio::sync::{broadcast, Mutex};

#[derive(Clone)]
pub struct ObservationBus { tx: broadcast::Sender<ObservationEvent>, history: Arc<Mutex<VecDeque<ObservationEvent>>>, capacity: usize }

impl ObservationBus {
    pub fn new(capacity: usize) -> Self { let (tx, _) = broadcast::channel(capacity.max(16)); Self { tx, history:Arc::new(Mutex::new(VecDeque::with_capacity(capacity))), capacity:capacity.max(16) } }
    pub fn subscribe(&self) -> broadcast::Receiver<ObservationEvent> { self.tx.subscribe() }
    pub async fn publish(&self, event: ObservationEvent) { let mut h=self.history.lock().await; if h.len()>=self.capacity { h.pop_front(); } h.push_back(event.clone()); drop(h); let _=self.tx.send(event); }
    pub async fn recent(&self, limit: usize) -> Vec<ObservationEvent> { self.history.lock().await.iter().rev().take(limit).cloned().collect::<Vec<_>>().into_iter().rev().collect() }
}

#[cfg(test)] mod tests { use super::*; use localview_protocol::{Endpoint, ObservationEvent}; use uuid::Uuid;
#[tokio::test] async fn bounds_history(){ let b=ObservationBus::new(16); for p in 1..=20 { b.publish(ObservationEvent::ServerDetected{session_id:Uuid::new_v4(),endpoint:Endpoint{host:"127.0.0.1".into(),port:p,scheme:"http".into()}}).await;} assert_eq!(b.recent(100).await.len(),16); }}
