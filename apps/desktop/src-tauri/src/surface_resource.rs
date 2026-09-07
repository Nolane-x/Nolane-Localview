#![forbid(unsafe_code)]

use localview_protocol::SessionId;
use serde::Serialize;
use uuid::Uuid;

use super::surface_registry::{DesktopSurfaceIdentity, DesktopSurfaceVisibility};

const SURFACE_RESOURCE_BASE: &str = "http://127.0.0.1:45454";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceReservationToken {
    session_id: SessionId,
    request_id: String,
}

#[derive(Debug, Serialize)]
struct SurfaceReservationRequest<'a> {
    session_id: SessionId,
    request_id: &'a str,
}

#[derive(Debug, Serialize)]
struct SurfaceActivateRequest<'a> {
    session_id: SessionId,
    request_id: &'a str,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    visibility: &'static str,
}

#[derive(Debug, Serialize)]
struct SurfaceVisibilityRequest<'a> {
    session_id: SessionId,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    visibility: &'static str,
}

#[derive(Debug, Serialize)]
struct SurfaceReleaseRequest<'a> {
    session_id: SessionId,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
}

pub async fn reserve_surface(session_id: SessionId) -> Result<SurfaceReservationToken, String> {
    let token = SurfaceReservationToken {
        session_id,
        request_id: format!("surface-{}", Uuid::new_v4()),
    };
    let request = SurfaceReservationRequest {
        session_id: token.session_id,
        request_id: &token.request_id,
    };
    post_surface("/v1/runtime/resources/surfaces/reserve", &request).await?;
    Ok(token)
}

pub async fn cancel_surface_reservation(token: &SurfaceReservationToken) -> Result<(), String> {
    let request = SurfaceReservationRequest {
        session_id: token.session_id,
        request_id: &token.request_id,
    };
    post_surface("/v1/runtime/resources/surfaces/cancel", &request).await
}

pub async fn activate_surface(
    token: &SurfaceReservationToken,
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
) -> Result<(), String> {
    if token.session_id != identity.session_id {
        return Err("surface reservation/session mismatch".into());
    }
    let request = SurfaceActivateRequest {
        session_id: identity.session_id,
        request_id: &token.request_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        visibility: runtime_visibility(visibility),
    };
    post_surface("/v1/runtime/resources/surfaces/activate", &request).await
}

pub async fn update_surface_visibility(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
) -> Result<(), String> {
    let request = SurfaceVisibilityRequest {
        session_id: identity.session_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        visibility: runtime_visibility(visibility),
    };
    post_surface("/v1/runtime/resources/surfaces/visibility", &request).await
}

pub async fn release_surface(identity: &DesktopSurfaceIdentity) -> Result<(), String> {
    let request = SurfaceReleaseRequest {
        session_id: identity.session_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
    };
    post_surface("/v1/runtime/resources/surfaces/release", &request).await
}

fn runtime_visibility(visibility: DesktopSurfaceVisibility) -> &'static str {
    match visibility {
        DesktopSurfaceVisibility::Visible => "visible",
        DesktopSurfaceVisibility::Hidden => "hidden",
    }
}

async fn post_surface<T: Serialize + ?Sized>(path: &str, body: &T) -> Result<(), String> {
    let token = super::super::read_token().await?;
    super::super::control_client()?
        .post(format!("{SURFACE_RESOURCE_BASE}{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    Ok(())
}
