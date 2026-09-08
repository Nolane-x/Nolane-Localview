#![forbid(unsafe_code)]

use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("required desktop surface resource source {} is unavailable: {error}", path.display())
    })
}

#[test]
fn surface_resource_client_uses_exact_authenticated_lifecycle_routes() {
    let resource = source("src/surface_resource.rs");

    for route in [
        "/v1/runtime/resources/surfaces/reserve",
        "/v1/runtime/resources/surfaces/cancel",
        "/v1/runtime/resources/surfaces/activate",
        "/v1/runtime/resources/surfaces/visibility",
        "/v1/runtime/resources/surfaces/release",
    ] {
        assert!(
            resource.contains(route),
            "desktop surface resource client must call exact lifecycle route {route}"
        );
    }

    assert!(
        resource.contains("super::super::read_token()"),
        "surface resource client must reuse the existing desktop control token reader"
    );
    assert!(
        resource.contains("super::super::control_client()?"),
        "surface resource client must reuse the existing desktop reqwest client"
    );
    assert!(
        resource.contains(".bearer_auth"),
        "every lifecycle request must authenticate against the existing control plane"
    );
    assert!(
        resource.contains("SurfaceReservationToken"),
        "resource admission must return an exact reservation token"
    );
    assert!(
        resource.contains("token.session_id != identity.session_id"),
        "activation must reject a token being replayed for another session"
    );
    assert!(
        resource.contains("Uuid::new_v4()"),
        "each surface admission attempt needs a distinct request id"
    );
}

#[test]
fn surface_resource_client_does_not_create_parallel_authority_or_aggregate_counts() {
    let resource = source("src/surface_resource.rs");
    let workspace = source("src/workspace_surface.rs");

    assert!(
        workspace.contains("#[path = \"surface_resource.rs\"]")
            && workspace.contains("pub mod surface_resource;"),
        "preview and workspace surfaces must share one desktop resource-client primitive"
    );
    assert!(
        !resource.contains("hidden_surfaces"),
        "desktop may not post a caller-owned hidden-surface aggregate"
    );
    assert!(
        !resource.contains("reqwest::Client::builder"),
        "surface resource client must not create a second HTTP client/authority path"
    );
    assert!(
        !resource.contains("control.token") && !resource.contains("dirs::"),
        "surface resource client must not invent a second credential discovery path"
    );
}

#[test]
fn desktop_owner_registration_threads_current_boot_proof_through_surface_protocol() {
    let resource = source("src/surface_resource.rs");
    let registry = source("src/surface_registry.rs");

    assert!(
        resource.contains("/v1/runtime/resources/surfaces/owners/register"),
        "desktop must explicitly register its process-lifetime surface owner with the daemon"
    );
    assert!(
        resource.contains("/v1/runtime/resources/surfaces/reattach"),
        "desktop must expose the exact reattach path for a platform surface surviving daemon restart"
    );
    assert!(
        resource.contains("DesktopSurfaceOwner"),
        "surface protocol needs one process-lifetime owner state instead of caller-supplied capabilities"
    );
    for proof_field in ["owner_instance_id", "boot_epoch", "owner_lease_id"] {
        assert!(
            resource.contains(proof_field),
            "every mutating surface request must be able to prove current owner field {proof_field}"
        );
    }
    assert!(
        registry.contains("primary_owner_instance_id")
            && resource.contains("DesktopSurfaceOwner::new(owner_instance_id)"),
        "network owner state must bind to the exact UUID published by the primary desktop surface registry"
    );
    assert!(
        resource.contains("tokio::sync::Mutex"),
        "registration refresh must serialize current-boot owner capability replacement"
    );
    assert!(
        !resource.contains("control.token") && !resource.contains("dirs::"),
        "owner registration must keep using the existing control credential path"
    );
}

#[test]
fn daemon_restart_recovery_is_whitelisted_exact_and_one_shot() {
    let resource = source("src/surface_resource.rs");

    for recoverable in [
        "surface_owner_not_registered",
        "surface_owner_boot_epoch_mismatch",
        "surface_owner_lease_mismatch",
    ] {
        assert!(
            resource.contains(recoverable),
            "daemon-restart recovery must explicitly whitelist {recoverable}"
        );
    }

    assert!(
        resource.contains("fn is_recoverable_owner_error"),
        "owner recovery must be gated by one explicit error-code whitelist"
    );
    assert!(
        resource.contains("refresh_registration"),
        "a recoverable stale-owner response must rotate the cached daemon registration"
    );
    assert!(
        resource.contains("reattach_surface_once"),
        "surviving platform surfaces must use exact current-owner reattach before mutation retry"
    );
    assert!(
        resource.contains("retry_visibility_once"),
        "visibility recovery must have a named one-shot retry seam rather than an unbounded loop"
    );

    for forbidden in [
        "surface_owner_fence_mismatch",
        "surface_owner_incarnation_mismatch",
        "surface_recovery_debt_missing",
        "resource_governor_denied",
    ] {
        assert!(
            !resource.contains(&format!("{forbidden} => true")),
            "non-owner-staleness error {forbidden} must never grant automatic re-registration/reattach"
        );
    }
}
