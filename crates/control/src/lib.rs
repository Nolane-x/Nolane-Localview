#![forbid(unsafe_code)]

use std::{net::SocketAddr, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use anyhow::Result;
use axum::{extract::{Path, State}, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::{get, post}, Json, Router};
use localview_observation::ObservationBus;
use localview_protocol::{Health, ObservationEvent, Session, SessionId};
use localview_sessions::SessionManager;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct ControlState { pub token: Arc<str>, pub sessions: Arc<SessionManager>, pub observations: ObservationBus, pub paused: Arc<AtomicBool> }

pub fn router(state: ControlState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session))
        .route("/v1/sessions/{id}/preview", post(set_preview))
        .route("/v1/events/recent", get(recent_events))
        .route("/v1/runtime/pause", post(pause))
        .route("/v1/runtime/resume", post(resume))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?; Ok(())
}

async fn health(State(s):State<ControlState>) -> Json<Health> { Json(Health{version:env!("CARGO_PKG_VERSION").into(),status:"ready".into(),paused:s.paused.load(Ordering::Relaxed),sessions:s.sessions.list().await.len()}) }

fn authorized(headers:&HeaderMap, state:&ControlState)->bool {
    let expected = format!("Bearer {}", state.token);
    headers.get(axum::http::header::AUTHORIZATION).and_then(|v|v.to_str().ok()).map(|v|v==expected).unwrap_or(false)
}
fn denied()->(StatusCode,Json<serde_json::Value>){(StatusCode::UNAUTHORIZED,Json(serde_json::json!({"error":"unauthorized"})))}

async fn list_sessions(State(s):State<ControlState>, headers:HeaderMap)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} Json(s.sessions.list().await).into_response() }
async fn get_session(State(s):State<ControlState>, headers:HeaderMap, Path(id):Path<SessionId>)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} match s.sessions.get(id).await {Some(v)=>Json(v).into_response(),None=>(StatusCode::NOT_FOUND,Json(serde_json::json!({"error":"session_not_found"}))).into_response()} }

#[derive(Debug,Deserialize)] struct PreviewRequest { visible: bool }
async fn set_preview(State(s):State<ControlState>, headers:HeaderMap, Path(id):Path<SessionId>, Json(req):Json<PreviewRequest>)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} if s.sessions.set_preview_visible(id,req.visible).await {StatusCode::NO_CONTENT}else{StatusCode::NOT_FOUND} }
async fn recent_events(State(s):State<ControlState>, headers:HeaderMap)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} Json(s.observations.recent(100).await).into_response() }
async fn pause(State(s):State<ControlState>, headers:HeaderMap)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} s.paused.store(true,Ordering::Relaxed); StatusCode::NO_CONTENT.into_response() }
async fn resume(State(s):State<ControlState>, headers:HeaderMap)->impl IntoResponse { if !authorized(&headers,&s){return denied().into_response();} s.paused.store(false,Ordering::Relaxed); StatusCode::NO_CONTENT.into_response() }

#[derive(Debug,Clone,Serialize,Deserialize)] pub struct EventEnvelope { pub event: ObservationEvent }
