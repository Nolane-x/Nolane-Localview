#![forbid(unsafe_code)]

use std::{sync::OnceLock, time::Duration};

use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::surface_registry::{
    DesktopSurfaceIdentity, DesktopSurfaceVisibility, primary_owner_instance_id,
};

const SURFACE_RESOURCE_BASE: &str = "http://127.0.0.1:45454";
const SURFACE_OWNER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

static SURFACE_OWNER: OnceLock<DesktopSurfaceOwner> = OnceLock::new();
static SURFACE_OWNER_HEARTBEAT_STARTED: OnceLock<()> = OnceLock::new();

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
    recovery: Mutex<()>,
}

impl DesktopSurfaceOwner {
    pub fn new(owner_instance_id: Uuid) -> Self {
        Self {
            owner_instance_id,
            registration: Mutex::new(None),
            recovery: Mutex::new(()),
        }
    }

    async fn registration(&self) -> Result<SurfaceOwnerRegistration, String> {
        let mut current = self.registration.lock().await;
        if let Some(registration) = *current {
            spawn_surface_owner_heartbeat();
            return Ok(registration);
        }
        let registration = register_surface_owner(self.owner_instance_id).await?;
        *current = Some(registration);
        spawn_surface_owner_heartbeat();
        Ok(registration)
    }

    async fn refresh_registration(
        &self,
        stale: SurfaceOwnerRegistration,
    ) -> Result<SurfaceOwnerRegistration, String> {
        let mut current = self.registration.lock().await;
        if let Some(registration) = *current {
            if registration != stale {
                return Ok(registration);
            }
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
struct SurfaceOwnerHeartbeatRequest {
    owner_instance_id: Uuid,
    boot_epoch: Uuid,
    owner_lease_id: Uuid,
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

#[derive(Debug, Deserialize)]
struct SurfaceErrorResponse {
    error: String,
}

#[derive(Debug)]
struct SurfaceControlError {
    code: Option<String>,
    message: String,
}

impl SurfaceControlError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    fn response(status: reqwest::StatusCode, code: Option<String>) -> Self {
        let message = match code.as_deref() {
            Some(code) => format!("surface control request failed with {status}: {code}"),
            None => format!("surface control request failed with {status}"),
        };
        Self { code, message }
    }

    fn into_string(self) -> String {
        self.message
    }
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
    reattach_surface_once(identity, visibility, proof)
        .await
        .map_err(SurfaceControlError::into_string)
}

pub async fn update_surface_visibility(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
) -> Result<(), String> {
    let proof = exact_identity_owner(identity).await?;
    match update_surface_visibility_once(identity, visibility, proof).await {
        Ok(()) => Ok(()),
        Err(error) if is_recoverable_owner_error(&error) => {
            retry_visibility_once(identity, visibility, proof).await
        }
        Err(error) => Err(error.into_string()),
    }
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

async fn retry_visibility_once(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
    stale: SurfaceOwnerRegistration,
) -> Result<(), String> {
    let owner = surface_owner()?;
    let _recovery = owner.recovery.lock().await;
    let current = owner.registration().await?;

    if current != stale {
        return update_surface_visibility_once(identity, visibility, current)
            .await
            .map_err(SurfaceControlError::into_string);
    }

    let proof = owner.refresh_registration(stale).await?;
    if identity.owner_instance_id != proof.owner_instance_id {
        return Err("surface identity/owner mismatch".into());
    }

    reattach_surface_once(identity, visibility, proof)
        .await
        .map_err(SurfaceControlError::into_string)?;
    update_surface_visibility_once(identity, visibility, proof)
        .await
        .map_err(SurfaceControlError::into_string)
}

async fn update_surface_visibility_once(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
    proof: SurfaceOwnerRegistration,
) -> Result<(), SurfaceControlError> {
    if identity.owner_instance_id != proof.owner_instance_id {
        return Err(SurfaceControlError::local(
            "surface identity/owner mismatch",
        ));
    }
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
    post_surface_response("/v1/runtime/resources/surfaces/visibility", &request).await
}

async fn reattach_surface_once(
    identity: &DesktopSurfaceIdentity,
    visibility: DesktopSurfaceVisibility,
    proof: SurfaceOwnerRegistration,
) -> Result<(), SurfaceControlError> {
    if identity.owner_instance_id != proof.owner_instance_id {
        return Err(SurfaceControlError::local(
            "surface identity/owner mismatch",
        ));
    }
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
    post_surface_response("/v1/runtime/resources/surfaces/reattach", &request).await
}

fn is_recoverable_owner_error(error: &SurfaceControlError) -> bool {
    matches!(
        error.code.as_deref(),
        Some("surface_owner_not_registered")
            | Some("surface_owner_boot_epoch_mismatch")
            | Some("surface_owner_lease_mismatch")
    )
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
    let owner_instance_id = primary_owner_instance_id().ok_or_else(|| {
        "desktop surface owner is unavailable before registry startup".to_string()
    })?;
    let owner = SURFACE_OWNER.get_or_init(|| DesktopSurfaceOwner::new(owner_instance_id));
    if owner.owner_instance_id != owner_instance_id {
        return Err("desktop surface owner does not match primary registry".into());
    }
    Ok(owner)
}

fn spawn_surface_owner_heartbeat() {
    if SURFACE_OWNER_HEARTBEAT_STARTED.set(()).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async {
        let mut interval = tokio::time::interval(SURFACE_OWNER_HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(owner) = SURFACE_OWNER.get() else {
                continue;
            };
            let _ = heartbeat_surface_owner_once(owner).await;
        }
    });
}

async fn heartbeat_surface_owner_once(owner: &DesktopSurfaceOwner) -> Result<(), String> {
    let proof = {
        let current = owner.registration.lock().await;
        (*current).ok_or_else(|| "surface owner registration unavailable".to_string())?
    };
    let request = SurfaceOwnerHeartbeatRequest {
        owner_instance_id: proof.owner_instance_id,
        boot_epoch: proof.boot_epoch,
        owner_lease_id: proof.owner_lease_id,
    };
    post_surface("/v1/runtime/resources/surfaces/owners/heartbeat", &request).await
}

async fn register_surface_owner(
    owner_instance_id: Uuid,
) -> Result<SurfaceOwnerRegistration, String> {
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
    post_surface_response(path, body)
        .await
        .map_err(SurfaceControlError::into_string)
}

async fn post_surface_response<T: Serialize + ?Sized>(
    path: &str,
    body: &T,
) -> Result<(), SurfaceControlError> {
    let token = super::super::read_token()
        .await
        .map_err(SurfaceControlError::local)?;
    let client = super::super::control_client().map_err(SurfaceControlError::local)?;
    let response = client
        .post(format!("{SURFACE_RESOURCE_BASE}{path}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|error| SurfaceControlError::local(error.to_string()))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let code = response
        .json::<SurfaceErrorResponse>()
        .await
        .ok()
        .map(|body| body.error);
    Err(SurfaceControlError::response(status, code))
}
