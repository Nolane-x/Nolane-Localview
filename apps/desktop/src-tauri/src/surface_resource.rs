#![forbid(unsafe_code)]

use std::sync::OnceLock;

use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::surface_registry::{
    primary_owner_instance_id, DesktopSurfaceIdentity, DesktopSurfaceVisibility,
};

const SURFACE_RESOURCE_BASE: &str = "http://127.0.0.1:45454";

static SURFACE_OWNER: OnceLock<DesktopSurfaceOwner> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct SurfaceOwnerRegistration {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
    #[serde(rename = "recovery_required")]
    _recovery_required: bool,
}

#[derive(Debug)]
pub struct DesktopSurfaceOwner {
    owner_instance_id: Uuid,
    registration: Mutex<Option<SurfaceOwnerRegistration>>,
}

impl DesktopSurfaceOwner {
    pub fn new(owner_instance_id: Uuid) -> Self {
        Self {
            owner_instance_id,
            registration: Mutex::new(None),
        }
    }

    async fn registration(&self) -> Result<SurfaceOwnerRegistration, String> {
        let mut current = self.registration.lock().await;
        if let Some(registration) = *current {
            return Ok(registration);
        }
        let registration = register_surface_owner(self.owner_instance_id).await?;
        *current = Some(registration);
        Ok(registration)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceReservationToken {
    session_id: SessionId,
    request_id: String,
    owner_instance_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceOwnerRegisterRequest {
    owner_instance_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceReservationRequest<'a> {
    session_id: SessionId,
    request_id: &'a str,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceActivateRequest<'a> {
    session_id: SessionId,
    request_id: &'a str,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    visibility: &'static str,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceVisibilityRequest<'a> {
    session_id: SessionId,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    visibility: &'static str,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceReleaseRequest<'a> {
    session_id: SessionId,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

#[derive(Debug, Serialize)]
struct SurfaceReattachRequest<'a> {
    session_id: SessionId,
    surface_kind: &'static str,
    label: &'a str,
    incarnation: u64,
    visibility: &'static str,
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
}

pub async fn reserve_surface(session_id: SessionId) -> Result<SurfaceReservationToken, String> {
    let proof = surface_owner()?.registration().await?;
    let token = SurfaceReservationToken {
        session_id,
        request_id: format!("surface-{}", Uuid::new_v4()),
        owner_instance_id: proof.owner_instance_id,
    };
    let request = SurfaceReservationRequest {
        session_id: token.session_id,
        request_id: &token.request_id,
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/reserve", &request).await?;
    Ok(token)
}

pub async fn cancel_surface_reservation(token: &SurfaceReservationToken) -> Result<(), String> {
    let proof = surface_owner()?.registration().await?;
    if token.owner_instance_id != proof.owner_instance_id {
        return Err("surface reservation/owner mismatch".into());
    }
    let request = SurfaceReservationRequest {
        session_id: token.session_id,
        request_id: &token.request_id,
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
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
    let proof = exact_identity_owner(identity).await?;
    if token.owner_instance_id != proof.owner_instance_id {
        return Err("surface reservation/owner mismatch".into());
    }
    let request = SurfaceActivateRequest {
        session_id: identity.session_id,
        request_id: &token.request_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        visibility: runtime_visibility(visibility),
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/activate", &request).await
}

pub async fn reattach_surface(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
) -> Result<(), String> {
    let proof = exact_identity_owner(identity).await?;
    let request = SurfaceReattachRequest {
        session_id: identity.session_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        visibility: runtime_visibility(visibility),
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/reattach", &request).await
}

pub async fn update_surface_visibility(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
) -> Result<(), String> {
    let proof = exact_identity_owner(identity).await?;
    let request = SurfaceVisibilityRequest {
        session_id: identity.session_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        visibility: runtime_visibility(visibility),
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/visibility", &request).await
}

pub async fn release_surface(identity: &DesktopSurfaceIdentity) -> Result<(), String> {
    let proof = exact_identity_owner(identity).await?;
    let request = SurfaceReleaseRequest {
        session_id: identity.session_id,
        surface_kind: identity.kind.as_runtime_kind(),
        label: &identity.label,
        incarnation: identity.incarnation,
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/release", &request).await
}

async fn exact_identity_owner(
    identity: &DesktopSurfaceIdentity,
) -> Result<SurfaceOwnerRegistration, String> {
    let proof = surface_owner()?.registration().await?;
    if identity.owner_instance_id != proof.owner_instance_id {
        return Err("surface identity/owner mismatch".into());
    }
    Ok(proof)
}

fn surface_owner() -> Result<&'static DesktopSurfaceOwner, String> {
    let owner_instance_id = primary_owner_instance_id()
        .ok_or_else(|| "desktop surface owner is unavailable before registry startup".to_string())?;
    let owner = SURFACE_OWNER.get_or_init(|| DesktopSurfaceOwner::new(owner_instance_id));
    if owner.owner_instance_id != owner_instance_id {
        return Err("desktop surface owner does not match primary registry".into());
    }
    Ok(owner)
}

async fn register_surface_owner(owner_instance_id: Uuid) -> Result<SurfaceOwnerRegistration, String> {
    let request = SurfaceOwnerRegisterRequest { owner_instance_id };
    let token = super::super::read_token().await?;
    let registration = super::super::control_client()?
        .post(format!(
            "{SURFACE_RESOURCE_BASE}/v1/runtime/resources/surfaces/owners/register"
        ))
        .bearer_auth(token)
        .json(&request)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<SurfaceOwnerRegistration>()
        .await
        .map_err(|error| error.to_string())?;
    if registration.owner_instance_id != owner_instance_id
        || registration.boot_epoch.is_nil()
        || registration.owner_lease_id.is_nil()
    {
        return Err("daemon returned invalid surface owner registration".into());
    }
    Ok(registration)
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
