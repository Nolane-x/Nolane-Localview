#![forbid(unsafe_code)]

use std::{
    collections::BTreeSet,
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
use chrono::{TimeZone, Utc};
use localview_evidence::{
    EvidenceDraft, EvidenceKind, EvidenceStore, Provenance, RetentionTier, UncertaintyClass,
};
use localview_live_analysis::{analyze_live, diagnose_live, FindingClass};
use localview_live_bridge::{
    BridgeAction, BridgeActionKind, BridgeActionResult, LiveBridge, ObserverBatch, ObserverEvent,
    ObserverEventKind,
};
use localview_observation::ObservationBus;
use localview_project_state::{inspect_git, ProjectRevision};
use localview_protocol::{Health, ObservationEvent as RuntimeObservationEvent, Session, SessionId};
use localview_security::SecretRedactor;
use localview_sessions::SessionManager;
use localview_verification::{
    proof_from_verification, proof_staleness, strict_coverage_report, verify_current, CoverageTarget,
    LiveVerificationPacket, LiveVerificationVerdict, StrictCoverageObservation, VerificationProof,
    VerificationState,
};
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

#[derive(Debug)]
enum ControlError {
    SessionNotFound,
    ProjectRootUnavailable,
    GitStateUnavailable(String),
}

impl IntoResponse for ControlError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::SessionNotFound => (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session_not_found"})),
            )
                .into_response(),
            Self::ProjectRootUnavailable => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "project_root_unavailable",
                    "message": "LocalView has no filesystem root for this session"
                })),
            )
                .into_response(),
            Self::GitStateUnavailable(message) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "error": "git_state_unavailable",
                    "message": message
                })),
            )
                .into_response(),
        }
    }
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
        .route("/v1/sessions/{id}/verify", get(session_verify))
        .route("/v1/sessions/{id}/coverage", get(session_coverage))
        .route("/v1/sessions/{id}/proof", post(create_session_proof))
        .route("/v1/sessions/{id}/evidence/recent", get(recent_evidence))
        .route(
            "/v1/sessions/{id}/evidence/visual",
            post(ingest_visual_evidence),
        )
        .route("/v1/evidence/{evidence_id}", get(get_evidence))
        .route("/v1/evidence/{evidence_id}/trace", get(trace_evidence))
        .route("/v1/proof/{evidence_id}/staleness", get(proof_evidence_staleness))
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

async fn ensure_session(state: &ControlState, id: SessionId) -> Result<(), ControlError> {
    if state.sessions.get(id).await.is_some() {
        Ok(())
    } else {
        Err(ControlError::SessionNotFound)
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
        None => ControlError::SessionNotFound.into_response(),
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
        return ControlError::SessionNotFound.into_response();
    };
    match project_revision(&session).await {
        Ok(revision) => Json(revision).into_response(),
        Err(error) => error.into_response(),
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
    }
    let events = state.live.recent(id, 2048).await;
    Json(diagnose_live(&events)).into_response()
}

async fn session_verify(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    match build_verification(&state, id).await {
        Ok(packet) => Json(packet).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn session_coverage(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let packet = match build_verification(&state, id).await {
        Ok(packet) => packet,
        Err(error) => return error.into_response(),
    };

    let required = packet.required_evidence_classes.clone();
    let target = CoverageTarget {
        id: "current_ui".into(),
        risk_weight: 1,
        required_evidence_classes: required.clone(),
    };
    let state_value = match packet.verdict {
        LiveVerificationVerdict::Pass => VerificationState::Verified,
        LiveVerificationVerdict::Stale => VerificationState::Stale,
        LiveVerificationVerdict::Fail | LiveVerificationVerdict::Inconclusive => {
            if packet.fresh_evidence_ids.is_empty() {
                VerificationState::Unknown
            } else {
                VerificationState::Observed
            }
        }
    };
    let observation = StrictCoverageObservation {
        target_id: "current_ui".into(),
        state: state_value,
        evidence_classes: packet.fresh_evidence_classes.clone(),
        evidence_ids: packet.fresh_evidence_ids.clone(),
    };
    let current_target = strict_coverage_report(&[target], &[observation]);
    Json(serde_json::json!({
        "project_denominator_known": false,
        "project_verified_ratio": serde_json::Value::Null,
        "reason": "LocalView has not compiled a complete product-state denominator for this project yet",
        "current_target": current_target,
    }))
    .into_response()
}

async fn create_session_proof(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let packet = match build_verification(&state, id).await {
        Ok(packet) => packet,
        Err(error) => return error.into_response(),
    };
    let proof = proof_from_verification(&packet);
    let draft = EvidenceDraft {
        kind: EvidenceKind::Proof,
        session_id: id,
        region: None,
        payload: serde_json::to_value(&proof).unwrap_or(serde_json::Value::Null),
        provenance: Provenance {
            source: "verification-runtime".into(),
            engine: Some("deterministic".into()),
            revision: proof.payload.revision.clone(),
            parent_ids: Vec::new(),
            captured_at: Utc::now(),
        },
        confidence: if proof.payload.verdict == LiveVerificationVerdict::Pass {
            1.0
        } else {
            0.0
        },
        uncertainty: if proof.payload.verdict == LiveVerificationVerdict::Pass {
            UncertaintyClass::Derived
        } else {
            UncertaintyClass::Unknown
        },
        secret_taint: false,
    };
    let stored = state
        .evidence
        .insert_with_retention(draft, RetentionTier::Project)
        .await;
    Json(serde_json::json!({
        "proof": proof,
        "evidence_id": stored.id,
        "deduplicated": stored.deduplicated,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualEvidenceRequest {
    artifact_id: String,
    pixel_width: u32,
    pixel_height: u32,
    backend: String,
    route: String,
    viewport: VisualViewport,
    revision: Option<String>,
    captured_at_unix_ms: i64,
    target: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VisualViewport {
    css_width: u32,
    css_height: u32,
    device_scale_factor: f64,
}

fn valid_visual_artifact_id(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("lv-") else {
        return false;
    };
    digest.len() == 16
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_visual_backend(value: &str) -> bool {
    matches!(value, "webview2" | "wk_web_view" | "web_kit_gtk")
}

fn valid_visual_route(value: &str) -> bool {
    let Ok(route) = url::Url::parse(value) else {
        return false;
    };
    if !matches!(route.scheme(), "http" | "https") {
        return false;
    }
    route.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

async fn ingest_visual_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<VisualEvidenceRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
    }

    if request.target != "viewport"
        || !valid_visual_artifact_id(&request.artifact_id)
        || !valid_visual_backend(&request.backend)
        || request.pixel_width == 0
        || request.pixel_height == 0
        || request.viewport.css_width == 0
        || request.viewport.css_height == 0
        || !request.viewport.device_scale_factor.is_finite()
        || request.viewport.device_scale_factor <= 0.0
        || !valid_visual_route(&request.route)
        || request.captured_at_unix_ms < 0
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_visual_evidence"})),
        )
            .into_response();
    }

    let Some(captured_at) = Utc.timestamp_millis_opt(request.captured_at_unix_ms).single() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid_capture_timestamp"})),
        )
            .into_response();
    };

    let payload = serde_json::json!({
        "artifact_id": request.artifact_id,
        "pixel_width": request.pixel_width,
        "pixel_height": request.pixel_height,
        "backend": request.backend,
        "route": request.route,
        "viewport": request.viewport,
        "target": "viewport",
    });
    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Visual,
            session_id: id,
            region: Some("viewport".into()),
            payload,
            provenance: Provenance {
                source: "native-capture".into(),
                engine: Some(request.backend),
                revision: request.revision,
                parent_ids: Vec::new(),
                captured_at,
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        })
        .await;

    Json(serde_json::json!({
        "evidence_id": stored.id,
        "deduplicated": stored.deduplicated,
    }))
    .into_response()
}

async fn recent_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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

async fn trace_evidence(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(evidence_id): Path<String>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.evidence.get(&evidence_id).await.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "evidence_not_found"})),
        )
            .into_response();
    }
    Json(state.evidence.trace(&evidence_id, 16).await).into_response()
}

async fn proof_evidence_staleness(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(evidence_id): Path<String>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    let Some(evidence) = state.evidence.get(&evidence_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "evidence_not_found"})),
        )
            .into_response();
    };
    if evidence.kind != EvidenceKind::Proof {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "evidence_is_not_proof"})),
        )
            .into_response();
    }
    let proof: VerificationProof = match serde_json::from_value(evidence.payload.clone()) {
        Ok(proof) => proof,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "invalid_proof_payload", "message": error.to_string()})),
            )
                .into_response();
        }
    };
    let current = match state.sessions.get(evidence.session_id).await {
        Some(session) => project_revision(&session).await.ok(),
        None => None,
    };
    Json(proof_staleness(
        &proof,
        current
            .as_ref()
            .map(|revision| revision.working_tree_id.as_str()),
    ))
    .into_response()
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
    }
    if request.action.is_internal_capture_action() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "internal_capture_action_not_public"
            })),
        )
            .into_response();
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
    }

    let Some(action) = state.live.claim_action(id, result.action_id).await else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "action_result_without_inflight_origin"})),
        )
            .into_response();
    };
    let revision = state
        .sessions
        .get(id)
        .await
        .and_then(|session| session.project.git_root.or(session.project.cwd));
    let revision = if let Some(root) = revision {
        inspect_git(root)
            .await
            .ok()
            .map(|value| value.working_tree_id)
    } else {
        None
    };

    let sanitized_result = sanitize_action_result(&action, &result);
    let interaction = EvidenceDraft {
        kind: EvidenceKind::Interaction,
        session_id: id,
        region: action.reference.clone(),
        payload: sanitized_result,
        provenance: Provenance {
            source: "native-action-executor".into(),
            engine: Some("native-webview".into()),
            revision: revision.clone(),
            parent_ids: Vec::new(),
            captured_at: result.completed_at,
        },
        confidence: 1.0,
        uncertainty: UncertaintyClass::Observed,
        secret_taint: false,
    };
    state.evidence.insert(interaction).await;

    if matches!(action.action, BridgeActionKind::Snapshot) && result.ok {
        for kind in [EvidenceKind::Semantic, EvidenceKind::Layout] {
            state
                .evidence
                .insert(EvidenceDraft {
                    kind,
                    session_id: id,
                    region: None,
                    payload: result.payload.clone(),
                    provenance: Provenance {
                        source: "native-semantic-snapshot".into(),
                        engine: Some("native-webview".into()),
                        revision: revision.clone(),
                        parent_ids: Vec::new(),
                        captured_at: result.completed_at,
                    },
                    confidence: 1.0,
                    uncertainty: UncertaintyClass::Observed,
                    secret_taint: false,
                })
                .await;
        }
    }

    state.live.complete_action(id, result).await;
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
    if let Err(error) = ensure_session(&state, id).await {
        return error.into_response();
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

async fn build_verification(
    state: &ControlState,
    id: SessionId,
) -> Result<LiveVerificationPacket, ControlError> {
    let Some(session) = state.sessions.get(id).await else {
        return Err(ControlError::SessionNotFound);
    };
    let revision = project_revision(&session).await.ok();
    let evidence = state.evidence.recent_for_session(id, 4096).await;
    let diagnosis = diagnose_live(&state.live.recent(id, 2048).await);
    let deterministic_failures = diagnosis
        .findings
        .iter()
        .filter(|finding| finding.class == FindingClass::Deterministic && finding.severity >= 3)
        .count();
    let required = BTreeSet::from(["semantic".to_owned(), "layout".to_owned()]);
    Ok(verify_current(
        revision
            .as_ref()
            .map(|value| value.working_tree_id.as_str()),
        &evidence,
        deterministic_failures,
        0,
        &required,
    ))
}

async fn project_revision(session: &Session) -> Result<ProjectRevision, ControlError> {
    let Some(root) = session
        .project
        .git_root
        .as_deref()
        .or(session.project.cwd.as_deref())
    else {
        return Err(ControlError::ProjectRootUnavailable);
    };
    inspect_git(root)
        .await
        .map_err(|error| ControlError::GitStateUnavailable(error.to_string()))
}

fn sanitize_action_result(action: &BridgeAction, result: &BridgeActionResult) -> serde_json::Value {
    let redactor = SecretRedactor::default();
    let error = result.error.as_deref().map(|value| redactor.redact(value));
    match &action.action {
        BridgeActionKind::TypeText { .. } => serde_json::json!({
            "action_id": result.action_id,
            "action": "type_text",
            "reference": action.reference,
            "ok": result.ok,
            "error": error,
            "completed_at": result.completed_at,
            "input_value_retained": false,
        }),
        BridgeActionKind::Snapshot => serde_json::json!({
            "action_id": result.action_id,
            "action": "snapshot",
            "ok": result.ok,
            "error": error,
            "completed_at": result.completed_at,
        }),
        BridgeActionKind::Click => action_summary(action, result, "click", error),
        BridgeActionKind::Key { .. } => action_summary(action, result, "key", error),
        BridgeActionKind::Scroll { .. } => action_summary(action, result, "scroll", error),
        BridgeActionKind::Focus => action_summary(action, result, "focus", error),
        BridgeActionKind::FreezeVisuals => {
            action_summary(action, result, "freeze_visuals", error)
        }
        BridgeActionKind::RestoreVisuals { .. } => {
            action_summary(action, result, "restore_visuals", error)
        }
    }
}

fn action_summary(
    action: &BridgeAction,
    result: &BridgeActionResult,
    action_name: &str,
    error: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "action_id": result.action_id,
        "action": action_name,
        "reference": action.reference,
        "ok": result.ok,
        "error": error,
        "completed_at": result.completed_at,
    })
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
        ObserverEventKind::DomMutation | ObserverEventKind::SemanticSnapshot => {
            EvidenceKind::Semantic
        }
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
    pub event: RuntimeObservationEvent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    fn action(kind: BridgeActionKind) -> BridgeAction {
        BridgeAction {
            id: Uuid::new_v4(),
            session_id: Uuid::nil(),
            reference: Some("@e1".into()),
            action: kind,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn type_action_evidence_never_retains_input_text_or_result_value() {
        let action = action(BridgeActionKind::TypeText {
            text: "super-secret-value".into(),
            clear_first: true,
        });
        let result = BridgeActionResult {
            action_id: action.id,
            ok: true,
            error: None,
            payload: serde_json::json!({"value":"super-secret-value"}),
            completed_at: Utc::now(),
        };
        let payload = sanitize_action_result(&action, &result).to_string();
        assert!(!payload.contains("super-secret-value"));
        assert!(payload.contains("input_value_retained"));
    }
}