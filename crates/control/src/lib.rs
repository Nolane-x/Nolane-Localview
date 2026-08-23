#![forbid(unsafe_code)]

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use localview_evidence::{
    EvidenceDraft, EvidenceKind, EvidenceStore, Provenance, UncertaintyClass,
};
use localview_live_analysis::{analyze_live, diagnose_live};
use localview_live_bridge::{
    BridgeActionKind, BridgeActionResult, LiveBridge, ObserverBatch, ObserverEvent,
    ObserverEventKind,
};
use localview_observation::ObservationBus;
use localview_project_state::inspect_git;
use localview_protocol::{Health, ObservationEvent, SessionId};
use localview_sessions::SessionManager;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct ControlState {
    pub token: Arc<str>,
    pub sessions: Arc<SessionManager>,
    pub observations: ObservationBus,
    pub live: LiveBridge,
    pub evidence: EvidenceStore,
    pub paused: Arc<AtomicBool>,
}

pub fn router(state: ControlState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/sessions", get(list_sessions))
        .route("/v1/sessions/{id}", get(get_session))
        .route("/v1/sessions/{id}/project-state", get(session_project_state))
        .route("/v1/sessions/{id}/preview", post(set_preview))
        .route("/v1/sessions/{id}/observer", post(ingest_observer))
        .route("/v1/sessions/{id}/observer/recent", get(recent_observer))
        .route("/v1/sessions/{id}/analysis", get(session_analysis))
        .route("/v1/sessions/{id}/diagnose", get(session_diagnose))
        .route("/v1/sessions/{id}/evidence/recent", get(recent_evidence))
        .route("/v1/evidence/{evidence_id}", get(get_evidence))
        .route("/v1/sessions/{id}/actions", post(queue_action).get(take_actions))
        .route(
            "/v1/sessions/{id}/actions/results",
            post(complete_action).get(action_results),
        )
        .route("/v1/events/recent", get(recent_events))
        .route("/v1/runtime/pause", post(pause))
        .route("/v1/runtime/resume", post(resume))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

async fn health(State(state): State<ControlState>) -> Json<Health> {
    Json(Health {
        version: env!("CARGO_PKG_VERSION").into(),
        status: "ready".into(),
        paused: state.paused.load(Ordering::Relaxed),
        sessions: state.sessions.list().await.len(),
    })
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn denied() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
        .into_response()
}

async fn ensure_session(
    state: &ControlState,
    id: SessionId,
) -> Result<(), axum::response::Response> {
    if state.sessions.get(id).await.is_some() {
        Ok(())
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response())
    }
}

async fn list_sessions(
    State(state): State<ControlState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    Json(state.sessions.list().await).into_response()
}

async fn get_session(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    match state.sessions.get(id).await {
        Some(value) => Json(value).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response(),
    }
}

async fn session_project_state(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let Some(session) = state.sessions.get(id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response();
    };
    let Some(root) = session.project.git_root.as_deref().or(session.project.cwd.as_deref()) else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "project_root_unavailable",
                "message": "LocalView has no filesystem root for this session"
            })),
        )
            .into_response();
    };
    match inspect_git(root).await {
        Ok(revision) => Json(revision).into_response(),
        Err(error) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "git_state_unavailable",
                "message": error.to_string()
            })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PreviewRequest {
    visible: bool,
}

async fn set_preview(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<PreviewRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.set_preview_visible(id, request.visible).await {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn ingest_observer(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(batch): Json<ObserverBatch>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    if batch.session_id != id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "session_id_mismatch"})),
        )
            .into_response();
    }

    let (report, accepted_events) = state.live.ingest_collect(batch).await;
    for event in accepted_events {
        state
            .evidence
            .insert(evidence_from_observer(id, report.generation, event))
            .await;
    }
    Json(report).into_response()
}

async fn recent_observer(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    Json(state.live.recent(id, 250).await).into_response()
}

async fn session_analysis(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    let events = state.live.recent(id, 2048).await;
    Json(analyze_live(&events)).into_response()
}

async fn session_diagnose(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    let events = state.live.recent(id, 2048).await;
    Json(diagnose_live(&events)).into_response()
}

async fn recent_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    Json(state.evidence.recent_for_session(id, 250).await).into_response()
}

async fn get_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(evidence_id): Path<String>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    match state.evidence.get(&evidence_id).await {
        Some(evidence) => Json(evidence).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "evidence_not_found"})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct QueueActionRequest {
    reference: Option<String>,
    action: BridgeActionKind,
}

async fn queue_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<QueueActionRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    let action = state
        .live
        .enqueue_action(id, request.reference, request.action)
        .await;
    (StatusCode::ACCEPTED, Json(action)).into_response()
}

async fn take_actions(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    Json(state.live.take_actions(id, 64).await).into_response()
}

async fn complete_action(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(result): Json<BridgeActionResult>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }

    let evidence = EvidenceDraft {
        kind: EvidenceKind::Interaction,
        session_id: id,
        region: None,
        payload: serde_json::to_value(&result).unwrap_or(serde_json::Value::Null),
        provenance: Provenance {
            source: "native-action-executor".into(),
            engine: Some("native-webview".into()),
            revision: None,
            parent_ids: Vec::new(),
            captured_at: result.completed_at,
        },
        confidence: 1.0,
        uncertainty: UncertaintyClass::Observed,
        secret_taint: false,
    };
    state.live.complete_action(id, result).await;
    state.evidence.insert(evidence).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn action_results(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(response) = ensure_session(&state, id).await {
        return response;
    }
    Json(state.live.recent_results(id, 100).await).into_response()
}

async fn recent_events(
    State(state): State<ControlState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    Json(state.observations.recent(100).await).into_response()
}

async fn pause(
    State(state): State<ControlState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    state.paused.store(true, Ordering::Relaxed);
    StatusCode::NO_CONTENT.into_response()
}

async fn resume(
    State(state): State<ControlState>,
    headers: HeaderMap,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    state.paused.store(false, Ordering::Relaxed);
    StatusCode::NO_CONTENT.into_response()
}

fn evidence_from_observer(
    session_id: SessionId,
    generation: u64,
    event: ObserverEvent,
) -> EvidenceDraft {
    let ObserverEvent {
        seq,
        captured_at,
        kind,
        reference,
        route,
        payload,
    } = event;
    let evidence_kind = evidence_kind(&kind);
    let region = reference.clone();
    EvidenceDraft {
        kind: evidence_kind,
        session_id,
        region,
        payload: serde_json::json!({
            "sequence": seq,
            "kind": kind,
            "route": route,
            "reference": reference,
            "observed": payload,
        }),
        provenance: Provenance {
            source: format!("preview-observer:generation:{generation}"),
            engine: Some("native-webview".into()),
            revision: None,
            parent_ids: Vec::new(),
            captured_at,
        },
        confidence: 1.0,
        uncertainty: UncertaintyClass::Observed,
        secret_taint: false,
    }
}

fn evidence_kind(kind: &ObserverEventKind) -> EvidenceKind {
    match kind {
        ObserverEventKind::DomMutation | ObserverEventKind::SemanticSnapshot => EvidenceKind::Semantic,
        ObserverEventKind::Layout => EvidenceKind::Layout,
        ObserverEventKind::Console | ObserverEventKind::RuntimeError => EvidenceKind::Console,
        ObserverEventKind::Network => EvidenceKind::Network,
        ObserverEventKind::Performance => EvidenceKind::Performance,
        ObserverEventKind::Route
        | ObserverEventKind::Focus
        | ObserverEventKind::Scroll
        | ObserverEventKind::Hmr => EvidenceKind::Interaction,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event: ObservationEvent,
}
