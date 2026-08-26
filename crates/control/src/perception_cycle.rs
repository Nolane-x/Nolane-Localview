#![forbid(unsafe_code)]

use std::time::Instant;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_engine::EngineDecision;
use localview_live_analysis::LiveDiagnosis;
use localview_planner::{
    perception_escalation_reason, BudgetedPerceptionPlan, PerceptionActionKind,
    PerceptionCycleSignals,
};
use localview_protocol::{PageSnapshot, SessionId};
use localview_token_budget::{
    evaluate_perception_budget, BudgetEscalationReason, PerceptionBudgetDecision,
    PerceptionBudgetUsage, PerceptionBudgetViolation,
};
use serde::Serialize;

use crate::{
    fresh_snapshot::{acquire_fresh_semantic_snapshot, FreshSnapshotError},
    perception::{
        authorized, build_live_perception_plan_with_usage, denied, plan_error_response,
        LivePerceptionPlanRequest,
    },
    ControlState,
};

const MAX_PERCEPTION_CYCLE_STEPS: usize = 4;

#[derive(Debug, Serialize)]
struct LivePerceptionCycleResponse {
    completion: PerceptionCycleCompletionReason,
    steps: Vec<PerceptionCycleStepReceipt>,
    usage: PerceptionBudgetUsage,
    budget_decision: PerceptionBudgetDecision,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum PerceptionCycleCompletionReason {
    NoOp,
}

#[derive(Debug, Serialize)]
struct PerceptionCycleStepReceipt {
    diagnosis: LiveDiagnosis,
    signals: PerceptionCycleSignals,
    plan: BudgetedPerceptionPlan,
    engine: Option<EngineDecision>,
    execution: PerceptionCycleExecutionReceipt,
    post_execution_budget_decision: PerceptionBudgetDecision,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PerceptionCycleExecutionReceipt {
    SemanticSnapshot { snapshot: PageSnapshot },
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/perception/cycle",
            post(execute_live_perception_cycle),
        )
        .with_state(state)
}

async fn execute_live_perception_cycle(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<LivePerceptionPlanRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }

    let started_at = Instant::now();
    let mut spent = zero_usage();
    let mut steps = Vec::new();
    let mut last_escalation_reason: Option<BudgetEscalationReason> = None;

    for _ in 0..MAX_PERCEPTION_CYCLE_STEPS {
        spent.latency_ms = elapsed_ms(started_at);

        let planned = match build_live_perception_plan_with_usage(&state, id, &request, &spent).await
        {
            Ok(planned) => planned,
            Err(error) => return plan_error_response(error),
        };

        let Some(selected) = planned.plan.actions.first().cloned() else {
            if !planned.plan.rejected.is_empty() {
                return budget_exhausted_response(spent, &planned.plan);
            }

            spent.latency_ms = elapsed_ms(started_at);
            let final_decision = match evaluate_perception_budget(
                &request.budget,
                &spent,
                last_escalation_reason,
            ) {
                Ok(decision) => decision,
                Err(violation) => return budget_violation_response(violation),
            };

            return Json(LivePerceptionCycleResponse {
                completion: PerceptionCycleCompletionReason::NoOp,
                steps,
                usage: spent,
                budget_decision: final_decision,
            })
            .into_response();
        };

        let action_kind = selected.action.kind;
        if action_kind != PerceptionActionKind::SemanticSnapshot {
            return executor_unavailable_response(action_kind, spent);
        }

        let execution = match acquire_fresh_semantic_snapshot(&state, id).await {
            Ok(snapshot) => PerceptionCycleExecutionReceipt::SemanticSnapshot { snapshot },
            Err(error) => return execution_error_response(error),
        };

        // The planner decision contains cumulative reservations for non-latency
        // dimensions. Replace its latency reservation with measured whole-cycle
        // elapsed time at the executor boundary instead of counting the forecast
        // as actual wall-clock use.
        spent = planned.plan.budget_decision.usage;
        spent.latency_ms = elapsed_ms(started_at);
        let escalation_reason = perception_escalation_reason(action_kind, &planned.signals);
        let post_execution_budget_decision = match evaluate_perception_budget(
            &request.budget,
            &spent,
            escalation_reason,
        ) {
            Ok(decision) => decision,
            Err(violation) => return budget_violation_response(violation),
        };
        last_escalation_reason = escalation_reason;

        steps.push(PerceptionCycleStepReceipt {
            diagnosis: planned.diagnosis,
            signals: planned.signals,
            plan: planned.plan,
            engine: planned.engine,
            execution,
            post_execution_budget_decision,
        });
    }

    step_limit_response(spent)
}

fn zero_usage() -> PerceptionBudgetUsage {
    PerceptionBudgetUsage {
        latency_ms: 0,
        text_tokens: 0,
        image_regions: 0,
        chromium_spawns: 0,
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn budget_exhausted_response(
    usage: PerceptionBudgetUsage,
    plan: &BudgetedPerceptionPlan,
) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "perception_budget_exhausted",
            "usage": usage,
            "rejected": plan.rejected,
        })),
    )
        .into_response()
}

fn budget_violation_response(violation: PerceptionBudgetViolation) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "perception_budget_exceeded",
            "budget": violation.budget,
            "usage": violation.usage,
            "exceeded": violation.exceeded,
        })),
    )
        .into_response()
}

fn executor_unavailable_response(
    kind: PerceptionActionKind,
    usage: PerceptionBudgetUsage,
) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "perception_executor_unavailable",
            "action_kind": kind,
            "usage": usage,
        })),
    )
        .into_response()
}

fn step_limit_response(usage: PerceptionBudgetUsage) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "perception_cycle_step_limit",
            "max_steps": MAX_PERCEPTION_CYCLE_STEPS,
            "usage": usage,
        })),
    )
        .into_response()
}

fn execution_error_response(error: FreshSnapshotError) -> axum::response::Response {
    let (status, code) = match error {
        FreshSnapshotError::SessionNotFound => (StatusCode::NOT_FOUND, "session_not_found"),
        FreshSnapshotError::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "perception_semantic_snapshot_timeout",
        ),
        FreshSnapshotError::Failed => (
            StatusCode::BAD_GATEWAY,
            "perception_semantic_snapshot_failed",
        ),
        FreshSnapshotError::Invalid => (
            StatusCode::BAD_GATEWAY,
            "invalid_perception_semantic_snapshot",
        ),
    };

    (status, Json(serde_json::json!({"error": code}))).into_response()
}
