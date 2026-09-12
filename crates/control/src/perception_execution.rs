#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_engine::EngineDecision;
use localview_live_analysis::LiveDiagnosis;
use localview_planner::{BudgetedPerceptionPlan, PerceptionActionKind, PerceptionCycleSignals};
use localview_protocol::{PageSnapshot, SessionId};
use serde::Serialize;

use crate::{
    fresh_snapshot::{acquire_fresh_semantic_snapshot, FreshSnapshotError},
    perception::{
        authorized, build_live_perception_plan, denied, plan_error_response,
        LivePerceptionPlanRequest, LivePerceptionPlanResponse,
    },
    ControlState,
};

#[derive(Debug, Serialize)]
struct LivePerceptionStepResponse {
    diagnosis: LiveDiagnosis,
    signals: PerceptionCycleSignals,
    plan: BudgetedPerceptionPlan,
    engine: Option<EngineDecision>,
    execution: Option<PerceptionExecutionReceipt>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PerceptionExecutionReceipt {
    SemanticSnapshot { snapshot: PageSnapshot },
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/perception/step",
            post(execute_live_perception_step),
        )
        .with_state(state)
}

async fn execute_live_perception_step(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(request): Json<LivePerceptionPlanRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }

    let planned = match build_live_perception_plan(&state, id, &request).await {
        Ok(planned) => planned,
        Err(error) => return plan_error_response(error),
    };

    let Some(selected) = planned.plan.actions.first() else {
        return Json(step_response(planned, None)).into_response();
    };

    match selected.action.kind {
        PerceptionActionKind::SemanticSnapshot => {
            match acquire_fresh_semantic_snapshot(&state, id).await {
                Ok(snapshot) => Json(step_response(
                    planned,
                    Some(PerceptionExecutionReceipt::SemanticSnapshot { snapshot }),
                ))
                .into_response(),
                Err(error) => execution_error_response(error),
            }
        }
        unsupported => executor_unavailable_response(unsupported),
    }
}

fn step_response(
    planned: LivePerceptionPlanResponse,
    execution: Option<PerceptionExecutionReceipt>,
) -> LivePerceptionStepResponse {
    LivePerceptionStepResponse {
        diagnosis: planned.diagnosis,
        signals: planned.signals,
        plan: planned.plan,
        engine: planned.engine,
        execution,
    }
}

fn executor_unavailable_response(kind: PerceptionActionKind) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": "perception_executor_unavailable",
            "action_kind": kind,
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
