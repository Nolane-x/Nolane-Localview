#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_engine::EngineDecision;
use localview_evidence::{EvidenceKind, EvidenceObject, UncertaintyClass};
use localview_live_analysis::LiveDiagnosis;
use localview_live_bridge::{NativeExecutorAction, NativeExecutorRequest, NativeExecutorResult};
use localview_planner::{
    perception_escalation_reason, BudgetedPerceptionPlan, PerceptionActionKind,
    PerceptionCycleSignals,
};
use localview_protocol::{PageSnapshot, SessionId, ViewportMeta};
use localview_token_budget::{
    evaluate_perception_budget, BudgetEscalationReason, PerceptionBudgetContract,
    PerceptionBudgetDecision, PerceptionBudgetUsage, PerceptionBudgetViolation,
};
use serde::Serialize;

use crate::{
    fresh_snapshot::{acquire_fresh_semantic_snapshot, FreshSnapshotError},
    perception::{
        authorized, build_live_perception_plan_with_usage_and_visual_satisfaction, denied,
        plan_error_response, LivePerceptionPlanRequest,
    },
    ControlState,
};

const MAX_PERCEPTION_CYCLE_STEPS: usize = 4;
const NATIVE_VISUAL_EXECUTOR_TIMEOUT: Duration = Duration::from_secs(12);
const NATIVE_VISUAL_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    SemanticSnapshot {
        snapshot: PageSnapshot,
    },
    NativeVisualPacket {
        request_id: uuid::Uuid,
        usage: PerceptionBudgetUsage,
        payload: serde_json::Value,
    },
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
    let mut visual_satisfied = false;

    for _ in 0..MAX_PERCEPTION_CYCLE_STEPS {
        spent.latency_ms = elapsed_ms(started_at);

        let planned = match build_live_perception_plan_with_usage_and_visual_satisfaction(
            &state,
            id,
            &request,
            &spent,
            visual_satisfied,
        )
        .await
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
        let escalation_reason = perception_escalation_reason(action_kind, &planned.signals);

        let execution = match action_kind {
            PerceptionActionKind::SemanticSnapshot => {
                let snapshot = match acquire_fresh_semantic_snapshot(&state, id).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => return execution_error_response(error),
                };

                // Semantic execution does not yet return a measured token receipt,
                // so retain the planner's cumulative non-latency reservation and
                // replace only latency with whole-cycle wall clock.
                spent = planned.plan.budget_decision.usage;
                spent.latency_ms = elapsed_ms(started_at);
                PerceptionCycleExecutionReceipt::SemanticSnapshot { snapshot }
            }
            PerceptionActionKind::RegionCapture => {
                let Some(viewport) = request.viewport.clone() else {
                    return visual_viewport_required_response();
                };
                let operation_budget = native_visual_operation_budget(&request.budget, &spent);
                let native_request = state
                    .live
                    .enqueue_native_executor(
                        id,
                        NativeExecutorAction::VisualPacket {
                            reference: selected.action.target.clone(),
                            viewport: viewport.clone(),
                            revision: request.revision.clone(),
                            budget: operation_budget,
                            budget_escalation_reason: escalation_reason,
                        },
                    )
                    .await;

                let result = match wait_for_native_executor_result(&state, id, native_request.id).await
                {
                    Ok(result) => result,
                    Err(()) => return native_visual_timeout_response(native_request.id),
                };
                if !result.ok {
                    return native_visual_failed_response(&result);
                }
                let Some(actual_usage) = result.usage.clone() else {
                    return native_visual_invalid_usage_response("native_visual_usage_missing");
                };
                if actual_usage.chromium_spawns != 0 || actual_usage.image_regions > 1 {
                    return native_visual_invalid_usage_response("native_visual_usage_invalid");
                }
                if !has_matching_native_visual_evidence(
                    &state,
                    id,
                    &native_request,
                    &viewport,
                    request.revision.as_deref(),
                )
                .await
                {
                    return native_visual_evidence_missing_response();
                }

                spent = add_actual_non_latency_usage(&spent, &actual_usage);
                spent.latency_ms = elapsed_ms(started_at);
                visual_satisfied = true;
                PerceptionCycleExecutionReceipt::NativeVisualPacket {
                    request_id: native_request.id,
                    usage: actual_usage,
                    payload: result.payload,
                }
            }
            _ => return executor_unavailable_response(action_kind, spent),
        };

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

fn native_visual_operation_budget(
    budget: &PerceptionBudgetContract,
    spent: &PerceptionBudgetUsage,
) -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: budget.latency_ms.saturating_sub(spent.latency_ms),
        text_tokens: budget.text_tokens.saturating_sub(spent.text_tokens),
        // A planner-authorized RegionCapture must be capable of producing one
        // image even when the original cycle needs an explicit escalation for
        // the cumulative overrun. The whole-cycle evaluator remains authority
        // for that cumulative decision after actual usage returns.
        image_regions: 1,
        chromium_spawns: 0,
    }
}

fn add_actual_non_latency_usage(
    spent: &PerceptionBudgetUsage,
    actual: &PerceptionBudgetUsage,
) -> PerceptionBudgetUsage {
    PerceptionBudgetUsage {
        latency_ms: spent.latency_ms,
        text_tokens: spent.text_tokens.saturating_add(actual.text_tokens),
        image_regions: spent.image_regions.saturating_add(actual.image_regions),
        chromium_spawns: spent
            .chromium_spawns
            .saturating_add(actual.chromium_spawns),
    }
}

async fn wait_for_native_executor_result(
    state: &ControlState,
    id: SessionId,
    request_id: uuid::Uuid,
) -> Result<NativeExecutorResult, ()> {
    tokio::time::timeout(NATIVE_VISUAL_EXECUTOR_TIMEOUT, async {
        loop {
            if let Some(result) = state
                .live
                .recent_native_executor_results(id, 16)
                .await
                .into_iter()
                .find(|result| result.request_id == request_id)
            {
                return result;
            }
            tokio::time::sleep(NATIVE_VISUAL_RESULT_POLL_INTERVAL).await;
        }
    })
    .await
    .map_err(|_| ())
}

async fn has_matching_native_visual_evidence(
    state: &ControlState,
    id: SessionId,
    native_request: &NativeExecutorRequest,
    viewport: &ViewportMeta,
    revision: Option<&str>,
) -> bool {
    state
        .evidence
        .recent_for_session(id, 64)
        .await
        .iter()
        .rev()
        .any(|evidence| {
            authoritative_native_visual_evidence(
                evidence,
                native_request,
                viewport,
                revision,
            )
        })
}

fn authoritative_native_visual_evidence(
    evidence: &EvidenceObject,
    native_request: &NativeExecutorRequest,
    viewport: &ViewportMeta,
    revision: Option<&str>,
) -> bool {
    if evidence.kind != EvidenceKind::Visual
        || evidence.provenance.source != "native-capture"
        || evidence.uncertainty != UncertaintyClass::Observed
        || evidence.confidence < 0.999
        || evidence.secret_taint
        || evidence.provenance.captured_at.timestamp_millis()
            < native_request.created_at.timestamp_millis()
        || !matches!(
            evidence.provenance.engine.as_deref(),
            Some("webview2" | "wk_web_view" | "web_kit_gtk")
        )
    {
        return false;
    }
    if revision.is_some() && evidence.provenance.revision.as_deref() != revision {
        return false;
    }

    let Some(evidence_viewport) = evidence.payload.get("viewport") else {
        return false;
    };
    let width_matches = evidence_viewport
        .get("css_width")
        .and_then(serde_json::Value::as_u64)
        == Some(u64::from(viewport.css_width));
    let height_matches = evidence_viewport
        .get("css_height")
        .and_then(serde_json::Value::as_u64)
        == Some(u64::from(viewport.css_height));
    let scale_matches = evidence_viewport
        .get("device_scale_factor")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|scale| (scale - viewport.device_scale_factor).abs() <= f64::EPSILON);
    width_matches && height_matches && scale_matches
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

fn visual_viewport_required_response() -> axum::response::Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({
            "error": "perception_visual_viewport_required"
        })),
    )
        .into_response()
}

fn native_visual_timeout_response(request_id: uuid::Uuid) -> axum::response::Response {
    (
        StatusCode::GATEWAY_TIMEOUT,
        Json(serde_json::json!({
            "error": "native_visual_executor_timeout",
            "request_id": request_id,
        })),
    )
        .into_response()
}

fn native_visual_failed_response(result: &NativeExecutorResult) -> axum::response::Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": "native_visual_executor_failed",
            "request_id": result.request_id,
            "reason": result.error,
        })),
    )
        .into_response()
}

fn native_visual_invalid_usage_response(code: &'static str) -> axum::response::Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({"error": code})),
    )
        .into_response()
}

fn native_visual_evidence_missing_response() -> axum::response::Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({"error": "native_visual_evidence_missing"})),
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
