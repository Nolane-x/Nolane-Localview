#![forbid(unsafe_code)]

use std::{sync::Arc, time::Instant};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use localview_sessions::SessionManager;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    perception::{authorized, denied},
    resource_runtime::release_surface_resource_owner_for_sessions,
    surface_owner::{
        heartbeat_surface_owner_for_sessions_at, reap_expired_surface_owners_for_sessions_at,
        SurfaceOwnerError, SurfaceOwnerProof,
    },
    ControlState,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceOwnerHeartbeatRequest {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

impl SurfaceOwnerHeartbeatRequest {
    fn owner_proof(&self) -> SurfaceOwnerProof {
        SurfaceOwnerProof {
            owner_instance_id: self.owner_instance_id,
            boot_epoch: self.boot_epoch,
            owner_lease_id: self.owner_lease_id,
        }
    }
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/runtime/resources/surfaces/owners/heartbeat",
            post(heartbeat_surface_owner),
        )
        .with_state(state)
}

async fn heartbeat_surface_owner(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(request): Json<SurfaceOwnerHeartbeatRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    match heartbeat_surface_owner_for_sessions_at(
        &state.sessions,
        request.owner_proof(),
        Instant::now(),
    ) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => surface_owner_conflict(error),
    }
}

pub fn reap_expired_surface_owner_resources_for_sessions_at(
    sessions: &Arc<SessionManager>,
    now: Instant,
) -> usize {
    reap_expired_surface_owners_for_sessions_at(sessions, now)
        .into_iter()
        .map(|owner_instance_id| {
            release_surface_resource_owner_for_sessions(sessions, owner_instance_id)
        })
        .sum()
}

pub fn reap_expired_surface_owner_resources_for_sessions(
    sessions: &Arc<SessionManager>,
) -> usize {
    reap_expired_surface_owner_resources_for_sessions_at(sessions, Instant::now())
}

fn surface_owner_conflict(error: SurfaceOwnerError) -> axum::response::Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({
            "error": match error {
                SurfaceOwnerError::NotRegistered => "surface_owner_not_registered",
                SurfaceOwnerError::BootEpochMismatch => "surface_owner_boot_epoch_mismatch",
                SurfaceOwnerError::LeaseMismatch => "surface_owner_lease_mismatch",
            }
        })),
    )
        .into_response()
}
