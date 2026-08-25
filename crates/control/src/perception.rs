#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_engine::{
    choose_engine_authorized, EngineAdmissionError, EngineDecision, EngineNeeds,
};
use localview_evidence::EvidenceKind;
use localview_live_analysis::{diagnose_live, LiveDiagnosis, LiveUncertaintyClass};
use localview_planner::{
    plan_budgeted_perception_cycle, BudgetedPerceptionCandidate, BudgetedPerceptionPlan,
    PerceptionActionKind, PerceptionCandidate, PerceptionCycleSignals,
};
use localview_protocol::SessionId;
use localview_token_budget::{PerceptionBudgetContract, PerceptionBudgetUsage};
use serde::{Deserialize, Serialize};

use crate::ControlState;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LivePerceptionPlanRequest {
    pub(crate) budget: PerceptionBudgetContract,
    #[serde(default)]
    pub(crate) deep_mode: bool,
    #[serde(default)]
    pub(crate) compatibility_requested: bool,
    pub(crate) target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LivePerceptionPlanResponse {
    pub(crate) diagnosis: LiveDiagnosis,
    pub(crate) signals: PerceptionCycleSignals,
    pub(crate) plan: BudgetedPerceptionPlan,
    pub(crate) engine: Option<EngineDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LivePerceptionPlanError {
    SessionNotFound,
    EngineAdmission(EngineAdmissionError),
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/perception/plan",
            post(plan_live_perception),
        )
        .with_state(state)
}

async fn plan_live_perception(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<LivePerceptionPlanRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }

    match build_live_perception_plan(&state, id, &request).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => plan_error_response(error),
    }
}

pub(crate) async fn build_live_perception_plan(
    state: &ControlState,
    id: SessionId,
    request: &LivePerceptionPlanRequest,
) -> Result<LivePerceptionPlanResponse, LivePerceptionPlanError> {
    if state.sessions.get(id).await.is_none() {
        return Err(LivePerceptionPlanError::SessionNotFound);
    }

    let events = state.live.recent(id, 2048).await;
    let diagnosis = diagnose_live(&events);
    let signals = derive_signals(&diagnosis, request);
    let candidates = derive_candidates(&diagnosis, &signals, request.target.as_deref());
    let plan = plan_budgeted_perception_cycle(&candidates, &request.budget, &signals);
    let engine = selected_engine(&plan).map_err(LivePerceptionPlanError::EngineAdmission)?;

    Ok(LivePerceptionPlanResponse {
        diagnosis,
        signals,
        plan,
        engine,
    })
}

pub(crate) fn plan_error_response(error: LivePerceptionPlanError) -> axum::response::Response {
    match error {
        LivePerceptionPlanError::SessionNotFound => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session_not_found"})),
        )
            .into_response(),
        LivePerceptionPlanError::EngineAdmission(reason) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "engine_admission_failed",
                "reason": reason,
            })),
        )
            .into_response(),
    }
}

fn derive_signals(
    diagnosis: &LiveDiagnosis,
    request: &LivePerceptionPlanRequest,
) -> PerceptionCycleSignals {
    let state_unknown = has_unknown(diagnosis, LiveUncertaintyClass::State);
    let visual_unknown = has_unknown(diagnosis, LiveUncertaintyClass::Visual);
    let critical_issue = diagnosis
        .findings
        .iter()
        .any(|finding| finding.severity >= 3 && finding.confidence >= 80);

    PerceptionCycleSignals {
        critical_issue,
        explicit_deep_mode: request.deep_mode,
        insufficient_evidence: !diagnosis.unknowns.is_empty(),
        // Compatibility intent is not authority by itself. Browser-specific
        // suspicion becomes actionable only after the cheaper semantic and
        // layout evidence needed to identify the page state is already known.
        browser_specific_suspicion: request.compatibility_requested
            && !state_unknown
            && !visual_unknown,
    }
}

fn derive_candidates(
    diagnosis: &LiveDiagnosis,
    signals: &PerceptionCycleSignals,
    target: Option<&str>,
) -> Vec<BudgetedPerceptionCandidate> {
    let mut candidates = Vec::new();
    let target = target.map(str::to_owned);
    let state_unknown = has_unknown(diagnosis, LiveUncertaintyClass::State);
    let visual_unknown = has_unknown(diagnosis, LiveUncertaintyClass::Visual);
    let cause_unknown = has_unknown(diagnosis, LiveUncertaintyClass::Cause);

    if state_unknown {
        candidates.push(candidate(
            "semantic-current-state",
            PerceptionActionKind::SemanticSnapshot,
            target.clone(),
            vec![EvidenceKind::Semantic],
            1.0,
            1.0,
            20,
            120,
            0,
            PerceptionBudgetUsage {
                latency_ms: 120,
                text_tokens: 120,
                image_regions: 0,
                chromium_spawns: 0,
            },
        ));
    }

    if visual_unknown {
        candidates.push(candidate(
            "visual-target-region",
            PerceptionActionKind::RegionCapture,
            target.clone(),
            vec![EvidenceKind::Visual, EvidenceKind::Layout],
            0.55,
            0.8,
            60,
            160,
            64 * 1024,
            PerceptionBudgetUsage {
                latency_ms: 300,
                text_tokens: 180,
                image_regions: 1,
                chromium_spawns: 0,
            },
        ));
    }

    if cause_unknown && !diagnosis.analysis.network.is_empty() {
        candidates.push(candidate(
            "network-cause-trace",
            PerceptionActionKind::NetworkRead,
            target.clone(),
            vec![EvidenceKind::Network, EvidenceKind::Causal],
            0.8,
            0.95,
            30,
            180,
            0,
            PerceptionBudgetUsage {
                latency_ms: 160,
                text_tokens: 220,
                image_regions: 0,
                chromium_spawns: 0,
            },
        ));
    }

    if cause_unknown && !diagnosis.analysis.console.is_empty() {
        candidates.push(candidate(
            "console-cause-trace",
            PerceptionActionKind::ConsoleRead,
            target.clone(),
            vec![EvidenceKind::Console, EvidenceKind::Causal],
            0.8,
            0.95,
            20,
            160,
            0,
            PerceptionBudgetUsage {
                latency_ms: 120,
                text_tokens: 180,
                image_regions: 0,
                chromium_spawns: 0,
            },
        ));
    }

    if signals.browser_specific_suspicion {
        candidates.push(candidate(
            "chromium-compatibility-check",
            PerceptionActionKind::ChromiumEscalation,
            target,
            vec![EvidenceKind::Visual],
            1.0,
            1.0,
            20,
            100,
            0,
            PerceptionBudgetUsage {
                latency_ms: 500,
                text_tokens: 200,
                image_regions: 0,
                // Planner owns normalization to exactly one spawn for a
                // ChromiumEscalation action.
                chromium_spawns: 0,
            },
        ));
    }

    candidates
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    id: &str,
    kind: PerceptionActionKind,
    target: Option<String>,
    expected_evidence: Vec<EvidenceKind>,
    uncertainty_reduction: f32,
    risk_relevance: f32,
    estimated_cpu_ms: u64,
    estimated_tokens: usize,
    estimated_capture_bytes: usize,
    estimated_usage: PerceptionBudgetUsage,
) -> BudgetedPerceptionCandidate {
    BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: id.into(),
            kind,
            target,
            expected_evidence,
            uncertainty_reduction,
            risk_relevance,
            estimated_cpu_ms,
            estimated_tokens,
            estimated_capture_bytes,
        },
        estimated_usage,
    }
}

fn selected_engine(
    plan: &BudgetedPerceptionPlan,
) -> Result<Option<EngineDecision>, EngineAdmissionError> {
    let Some(selected) = plan.actions.first() else {
        return Ok(None);
    };

    let needs = match selected.action.kind {
        PerceptionActionKind::ChromiumEscalation => EngineNeeds {
            chrome_compatibility: true,
            ..Default::default()
        },
        PerceptionActionKind::RegionCapture
        | PerceptionActionKind::ViewportCapture
        | PerceptionActionKind::ResponsiveSweep => EngineNeeds {
            screenshot: true,
            ..Default::default()
        },
        PerceptionActionKind::InteractionReplay => EngineNeeds {
            interaction: true,
            ..Default::default()
        },
        PerceptionActionKind::SemanticSnapshot
        | PerceptionActionKind::ElementInspect
        | PerceptionActionKind::ConsoleRead
        | PerceptionActionKind::NetworkRead
        | PerceptionActionKind::AccessibilityScan
        | PerceptionActionKind::PerformanceSample => EngineNeeds {
            javascript: true,
            ..Default::default()
        },
    };

    choose_engine_authorized(&needs, Some(plan)).map(Some)
}

fn has_unknown(diagnosis: &LiveDiagnosis, class: LiveUncertaintyClass) -> bool {
    diagnosis.unknowns.iter().any(|unknown| unknown.class == class)
}

pub(crate) fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

pub(crate) fn denied() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
        .into_response()
}
