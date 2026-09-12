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
use localview_evidence::{EvidenceKind, EvidenceObject, UncertaintyClass};
use localview_live_analysis::{diagnose_live, LiveDiagnosis, LiveUncertaintyClass};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_planner::{
    plan_budgeted_perception_cycle_with_usage, BudgetedPerceptionCandidate, BudgetedPerceptionPlan,
    PerceptionActionKind, PerceptionCandidate, PerceptionCycleSignals,
};
use localview_protocol::{SessionId, ViewportMeta};
use localview_resource_governor::{ResourceAdmissionDenial, ResourceWorkKind};
use localview_token_budget::{PerceptionBudgetContract, PerceptionBudgetUsage};
use serde::{Deserialize, Serialize};

use crate::{
    chromium_runtime::canonical_chromium_route_identity,
    resource_runtime::{denial_response as resource_denial_response, governor as resource_governor},
    ControlState,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LivePerceptionPlanRequest {
    pub(crate) budget: PerceptionBudgetContract,
    #[serde(default)]
    pub(crate) deep_mode: bool,
    #[serde(default)]
    pub(crate) compatibility_requested: bool,
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) viewport: Option<ViewportMeta>,
    #[serde(default)]
    pub(crate) revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LivePerceptionPlanResponse {
    pub(crate) diagnosis: LiveDiagnosis,
    pub(crate) signals: PerceptionCycleSignals,
    pub(crate) plan: BudgetedPerceptionPlan,
    pub(crate) engine: Option<EngineDecision>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LivePerceptionPlanError {
    SessionNotFound,
    EngineAdmission(EngineAdmissionError),
    ResourceGovernor(ResourceAdmissionDenial),
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
    let zero = PerceptionBudgetUsage {
        latency_ms: 0,
        text_tokens: 0,
        image_regions: 0,
        chromium_spawns: 0,
    };
    build_live_perception_plan_with_usage(state, id, request, &zero).await
}

pub(crate) async fn build_live_perception_plan_with_usage(
    state: &ControlState,
    id: SessionId,
    request: &LivePerceptionPlanRequest,
    spent: &PerceptionBudgetUsage,
) -> Result<LivePerceptionPlanResponse, LivePerceptionPlanError> {
    build_live_perception_plan_with_usage_and_visual_satisfaction(
        state, id, request, spent, false,
    )
    .await
}

pub(crate) async fn build_live_perception_plan_with_usage_and_visual_satisfaction(
    state: &ControlState,
    id: SessionId,
    request: &LivePerceptionPlanRequest,
    spent: &PerceptionBudgetUsage,
    visual_satisfied: bool,
) -> Result<LivePerceptionPlanResponse, LivePerceptionPlanError> {
    let Some(session) = state.sessions.get(id).await else {
        return Err(LivePerceptionPlanError::SessionNotFound);
    };

    let mut events = state.live.recent(id, 2048).await;
    let current_route = session.endpoint.url().ok().and_then(|base| {
        events
            .iter()
            .rev()
            .find_map(|event| event.route.as_deref())
            .and_then(|route| canonical_chromium_route_identity(&base, route))
    });
    let retained = state.evidence.recent_for_session(id, 64).await;
    append_retained_snapshot_observations(&mut events, &retained);
    let diagnosis = diagnose_live(&events);
    let chromium_satisfied = retained.iter().rev().any(|evidence| {
        authoritative_chromium_compatibility(
            evidence,
            request.revision.as_deref(),
            current_route.as_deref(),
        )
    });
    let signals = derive_signals(&diagnosis, request, visual_satisfied, chromium_satisfied);
    let candidates = derive_candidates(
        &diagnosis,
        &signals,
        request.target.as_deref(),
        visual_satisfied,
    );
    let plan = plan_budgeted_perception_cycle_with_usage(
        &candidates,
        &request.budget,
        spent,
        &signals,
    );
    if plan.actions.first().is_some_and(|selected| {
        selected.action.kind == PerceptionActionKind::ChromiumEscalation
    }) {
        resource_governor(state)
            .check(ResourceWorkKind::Chromium)
            .map_err(LivePerceptionPlanError::ResourceGovernor)?;
    }
    let engine = selected_engine(&plan).map_err(LivePerceptionPlanError::EngineAdmission)?;

    Ok(LivePerceptionPlanResponse {
        diagnosis,
        signals,
        plan,
        engine,
    })
}

fn append_retained_snapshot_observations(
    events: &mut Vec<ObserverEvent>,
    retained: &[EvidenceObject],
) {
    let mut next_seq = events.iter().map(|event| event.seq).max().unwrap_or(0);

    for (evidence_kind, event_kind) in [
        (EvidenceKind::Semantic, ObserverEventKind::SemanticSnapshot),
        (EvidenceKind::Layout, ObserverEventKind::Layout),
    ] {
        if events.iter().any(|event| event.kind == event_kind) {
            continue;
        }

        let Some(evidence) = retained
            .iter()
            .rev()
            .find(|evidence| authoritative_native_snapshot(evidence, evidence_kind))
        else {
            continue;
        };

        next_seq = next_seq.saturating_add(1);
        events.push(ObserverEvent {
            seq: next_seq,
            captured_at: evidence.provenance.captured_at,
            kind: event_kind,
            reference: evidence.region.clone(),
            route: evidence
                .payload
                .get("route")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            payload: evidence.payload.clone(),
        });
    }
}

fn authoritative_native_snapshot(evidence: &EvidenceObject, kind: EvidenceKind) -> bool {
    evidence.kind == kind
        && evidence.provenance.source == "native-semantic-snapshot"
        && evidence.provenance.engine.as_deref() == Some("native-webview")
        && evidence.uncertainty == UncertaintyClass::Observed
        && evidence.confidence >= 0.999
        && !evidence.secret_taint
}

fn authoritative_chromium_compatibility(
    evidence: &EvidenceObject,
    revision: Option<&str>,
    current_route: Option<&str>,
) -> bool {
    evidence.kind == EvidenceKind::Contract
        && evidence.provenance.source == "chromium-compatibility"
        && evidence.provenance.engine.as_deref() == Some("chromium")
        && evidence.uncertainty == UncertaintyClass::Observed
        && evidence.confidence >= 0.999
        && !evidence.secret_taint
        && evidence
            .payload
            .get("probe")
            .and_then(serde_json::Value::as_str)
            == Some("page_load_dump_dom")
        && (revision.is_none() || evidence.provenance.revision.as_deref() == revision)
        && current_route.is_some_and(|route| {
            evidence
                .payload
                .get("target")
                .and_then(serde_json::Value::as_str)
                == Some(route)
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
        LivePerceptionPlanError::ResourceGovernor(denial) => resource_denial_response(denial),
    }
}

fn derive_signals(
    diagnosis: &LiveDiagnosis,
    request: &LivePerceptionPlanRequest,
    visual_satisfied: bool,
    chromium_satisfied: bool,
) -> PerceptionCycleSignals {
    let state_unknown = has_unknown(diagnosis, LiveUncertaintyClass::State);
    let visual_unknown = has_unknown(diagnosis, LiveUncertaintyClass::Visual) && !visual_satisfied;
    let critical_issue = diagnosis
        .findings
        .iter()
        .any(|finding| finding.severity >= 3 && finding.confidence >= 80);
    let insufficient_evidence = diagnosis
        .unknowns
        .iter()
        .any(|unknown| !(visual_satisfied && unknown.class == LiveUncertaintyClass::Visual));

    PerceptionCycleSignals {
        critical_issue,
        explicit_deep_mode: request.deep_mode,
        insufficient_evidence,
        browser_specific_suspicion: request.compatibility_requested
            && !state_unknown
            && !visual_unknown
            && !chromium_satisfied,
    }
}

fn derive_candidates(
    diagnosis: &LiveDiagnosis,
    signals: &PerceptionCycleSignals,
    target: Option<&str>,
    visual_satisfied: bool,
) -> Vec<BudgetedPerceptionCandidate> {
    let mut candidates = Vec::new();
    let target = target.map(str::to_owned);
    let state_unknown = has_unknown(diagnosis, LiveUncertaintyClass::State);
    let visual_unknown = has_unknown(diagnosis, LiveUncertaintyClass::Visual) && !visual_satisfied;
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
            vec![EvidenceKind::Visual],
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
            vec![EvidenceKind::Contract],
            1.0,
            1.0,
            20,
            100,
            0,
            PerceptionBudgetUsage {
                latency_ms: 500,
                text_tokens: 200,
                image_regions: 0,
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
    diagnosis
        .unknowns
        .iter()
        .any(|unknown| unknown.class == class)
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
