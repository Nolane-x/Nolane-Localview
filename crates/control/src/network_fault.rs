#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, post},
};
use chrono::{Duration as ChronoDuration, Utc};
use localview_live_bridge::{
    NetworkFaultControlCommand, NetworkFaultControlResult, NetworkFaultLeaseAuthority,
};
use localview_network::{NetworkFaultPlan, canonicalize_fault_plan};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::ControlState;

const CONTROL_ACK_TIMEOUT: Duration = Duration::from_millis(2_500);
const CONTROL_ACK_POLL: Duration = Duration::from_millis(20);
const MAX_PRIVATE_CONTROL_DRAIN: usize = 8;
const MAX_INSTALL_BODY_BYTES: usize = 16 * 1024;

fn mutation_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Serialize)]
struct NetworkFaultLeaseView {
    active: bool,
    lease_id: Uuid,
    fingerprint: String,
    rule_count: usize,
    expires_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewInvalidation {
    surface_incarnation: u64,
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/network-faults",
            post(install_network_faults).get(network_fault_status),
        )
        .route(
            "/v1/sessions/{id}/network-faults/{lease_id}",
            delete(clear_network_faults),
        )
        .route(
            "/v1/sessions/{id}/network-faults/invalidate-preview",
            post(invalidate_preview_fault_lease),
        )
        .route(
            "/v1/sessions/{id}/network-fault-controls",
            get(take_network_fault_controls),
        )
        .route(
            "/v1/sessions/{id}/network-fault-controls/results",
            post(complete_network_fault_control),
        )
        .with_state(state)
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token.as_ref())
}

fn bounded_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

async fn ensure_session(state: &ControlState, id: SessionId) -> bool {
    state.sessions.get(id).await.is_some()
}

async fn wait_for_result(
    state: &ControlState,
    session_id: SessionId,
    request_id: Uuid,
) -> Option<NetworkFaultControlResult> {
    let deadline = tokio::time::Instant::now() + CONTROL_ACK_TIMEOUT;
    loop {
        if let Some(result) = state
            .live
            .network_fault_control_result(session_id, request_id)
            .await
        {
            return Some(result);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(CONTROL_ACK_POLL).await;
    }
}

fn lease_view(lease: NetworkFaultLeaseAuthority) -> NetworkFaultLeaseView {
    NetworkFaultLeaseView {
        active: true,
        lease_id: lease.lease_id,
        fingerprint: lease.fingerprint,
        rule_count: lease.rule_count,
        expires_at: lease.expires_at,
    }
}

async fn current_live_lease(
    state: &ControlState,
    session_id: SessionId,
) -> Option<NetworkFaultLeaseAuthority> {
    let lease = state.live.network_fault_lease(session_id).await?;
    if lease.expires_at <= Utc::now() {
        let _ = state
            .live
            .clear_network_fault_lease(session_id, lease.lease_id)
            .await;
        None
    } else {
        Some(lease)
    }
}

async fn queue_failed_install_cleanup(
    state: &ControlState,
    session_id: SessionId,
    lease_token: Uuid,
    previous_lease: Option<NetworkFaultLeaseAuthority>,
    clear_previous_runtime: bool,
) {
    state
        .live
        .enqueue_network_fault_control(
            session_id,
            NetworkFaultControlCommand::Clear { lease_token },
        )
        .await;

    if clear_previous_runtime {
        if let Some(previous) = previous_lease.as_ref() {
            state
                .live
                .enqueue_network_fault_control(
                    session_id,
                    NetworkFaultControlCommand::Clear {
                        lease_token: previous.lease_token,
                    },
                )
                .await;
        }
    }

    if let Some(previous) = previous_lease {
        let _ = state
            .live
            .clear_network_fault_lease(session_id, previous.lease_id)
            .await;
    }
}

async fn install_network_faults(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }
    if body.len() > MAX_INSTALL_BODY_BYTES {
        return bounded_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "network_fault_plan_too_large",
        );
    }
    let plan: NetworkFaultPlan = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bounded_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "network_fault_invalid_schema",
            );
        }
    };

    let canonical = match canonicalize_fault_plan(&plan) {
        Ok(value) => value,
        Err(_) => {
            return bounded_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "network_fault_invalid_plan",
            );
        }
    };
    let canonical_value = match serde_json::to_value(&canonical) {
        Ok(value) => value,
        Err(_) => {
            return bounded_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "network_fault_plan_encoding_failed",
            );
        }
    };

    let _gate = mutation_gate().lock().await;
    let previous_lease = current_live_lease(&state, id).await;
    let lease_id = Uuid::new_v4();
    let lease_token = Uuid::new_v4();
    let request = state
        .live
        .enqueue_network_fault_control(
            id,
            NetworkFaultControlCommand::Install {
                lease_token,
                plan: canonical_value,
            },
        )
        .await;

    let Some(result) = wait_for_result(&state, id, request.id).await else {
        queue_failed_install_cleanup(&state, id, lease_token, previous_lease, true).await;
        return bounded_error(
            StatusCode::GATEWAY_TIMEOUT,
            "network_fault_preview_ack_timeout",
        );
    };
    if !result.ok {
        queue_failed_install_cleanup(&state, id, lease_token, previous_lease, true).await;
        return bounded_error(StatusCode::BAD_GATEWAY, "network_fault_preview_rejected");
    }

    let payload = &result.payload;
    let active = payload.get("active").and_then(Value::as_bool) == Some(true);
    let fingerprint = payload.get("fingerprint").and_then(Value::as_str);
    let rule_count = payload.get("rule_count").and_then(Value::as_u64);
    let remaining_ms = payload
        .get("remaining_ms")
        .or_else(|| payload.get("expires_in_ms"))
        .and_then(Value::as_u64);
    let surface_incarnation = payload.get("surface_incarnation").and_then(Value::as_u64);
    if !active
        || fingerprint != Some(canonical.fingerprint.as_str())
        || rule_count != Some(canonical.rules.len() as u64)
        || !remaining_ms.is_some_and(|value| value > 0 && value <= canonical.lease_ms)
        || !surface_incarnation.is_some_and(|value| value > 0)
    {
        queue_failed_install_cleanup(&state, id, lease_token, previous_lease, false).await;
        return bounded_error(
            StatusCode::BAD_GATEWAY,
            "network_fault_preview_ack_mismatch",
        );
    }

    let remaining_ms = remaining_ms.expect("validated above");
    let expires_at =
        Utc::now() + ChronoDuration::milliseconds(i64::try_from(remaining_ms).unwrap_or(i64::MAX));
    let lease = NetworkFaultLeaseAuthority {
        lease_id,
        lease_token,
        fingerprint: canonical.fingerprint,
        rule_count: canonical.rules.len(),
        surface_incarnation: surface_incarnation.expect("validated above"),
        expires_at,
    };
    state.live.set_network_fault_lease(id, lease.clone()).await;

    (StatusCode::CREATED, Json(lease_view(lease))).into_response()
}

async fn network_fault_status(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }

    match current_live_lease(&state, id).await {
        Some(lease) => {
            Json(serde_json::to_value(lease_view(lease)).unwrap_or(Value::Null)).into_response()
        }
        None => Json(serde_json::json!({"active": false})).into_response(),
    }
}

async fn clear_network_faults(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, lease_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }

    let _gate = mutation_gate().lock().await;
    let Some(lease) = current_live_lease(&state, id).await else {
        return bounded_error(StatusCode::NOT_FOUND, "network_fault_lease_not_found");
    };
    if lease.lease_id != lease_id {
        return bounded_error(StatusCode::NOT_FOUND, "network_fault_lease_not_found");
    }

    let request = state
        .live
        .enqueue_network_fault_control(
            id,
            NetworkFaultControlCommand::Clear {
                lease_token: lease.lease_token,
            },
        )
        .await;
    let Some(result) = wait_for_result(&state, id, request.id).await else {
        return bounded_error(
            StatusCode::GATEWAY_TIMEOUT,
            "network_fault_clear_ack_timeout",
        );
    };
    if !result.ok
        || result.payload.get("active").and_then(Value::as_bool) != Some(false)
        || result
            .payload
            .get("surface_incarnation")
            .and_then(Value::as_u64)
            != Some(lease.surface_incarnation)
    {
        return bounded_error(StatusCode::BAD_GATEWAY, "network_fault_clear_rejected");
    }

    if !state.live.clear_network_fault_lease(id, lease_id).await {
        return bounded_error(StatusCode::CONFLICT, "network_fault_lease_changed");
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn invalidate_preview_fault_lease(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }
    if body.len() > 1024 {
        return bounded_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "network_fault_invalidation_too_large",
        );
    }
    let invalidation: PreviewInvalidation = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return bounded_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "network_fault_invalidation_invalid",
            );
        }
    };
    if invalidation.surface_incarnation == 0 {
        return bounded_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "network_fault_invalidation_invalid",
        );
    }

    if let Some(lease) = state.live.network_fault_lease(id).await {
        if lease.surface_incarnation == invalidation.surface_incarnation {
            let _ = state
                .live
                .clear_network_fault_lease(id, lease.lease_id)
                .await;
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn take_network_fault_controls(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }

    Json(
        state
            .live
            .take_network_fault_controls(id, MAX_PRIVATE_CONTROL_DRAIN)
            .await,
    )
    .into_response()
}

async fn complete_network_fault_control(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
    Json(result): Json<NetworkFaultControlResult>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if !ensure_session(&state, id).await {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }
    let Some(request) = state
        .live
        .claim_network_fault_control(id, result.request_id)
        .await
    else {
        return bounded_error(
            StatusCode::CONFLICT,
            "network_fault_result_without_private_origin",
        );
    };
    if request.session_id != id {
        return bounded_error(StatusCode::CONFLICT, "network_fault_owner_mismatch");
    }
    if !state.live.complete_network_fault_control(id, result).await {
        return bounded_error(StatusCode::CONFLICT, "network_fault_completion_rejected");
    }
    StatusCode::NO_CONTENT.into_response()
}
