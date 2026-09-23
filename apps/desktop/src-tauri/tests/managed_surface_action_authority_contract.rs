use std::fs;

fn source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file must be readable")
}

fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).expect("start marker must exist");
    let tail = &source[start..];
    let end = tail.find(end).unwrap_or(tail.len());
    &tail[..end]
}

#[test]
fn public_actions_are_bound_to_exact_primary_managed_surface() {
    let control = source("../../../crates/control/src/resource_runtime.rs");
    let desktop = source("src/lib.rs");
    let surface = source("src/surface_resource.rs");

    let primary = between(
        &control,
        "fn primary_live_surface(",
        "fn validate_primary_surface_action_request(",
    );
    let preview = primary.find(r#""preview_window""#).expect("preview priority");
    let workspace = primary
        .find(r#""workspace_child""#)
        .expect("workspace fallback");
    assert!(preview < workspace, "preview must remain the primary action surface");

    let validate = between(
        &control,
        "fn validate_primary_surface_action_request(",
        "async fn ensure_managed_surface_observation_binding(",
    );
    for required in [
        "proof.owner_instance_id",
        "entry.live.contains_key(&exact_key)",
        "surface_not_primary_action_authority",
        "managed_surface_refs",
    ] {
        assert!(validate.contains(required), "missing exact surface fence: {required}");
    }

    let take = between(
        &control,
        "async fn take_surface_actions(",
        "async fn complete_surface_action(",
    );
    assert!(take.contains("pin_surface_owner_for_sessions"));
    assert!(take.contains("ensure_managed_surface_observation_binding"));
    assert!(take.contains("take_public_actions(request.session_id, 16)"));

    let complete = between(
        &control,
        "async fn complete_surface_action(",
        "async fn release_surface_resource(",
    );
    assert!(complete.contains("pin_surface_owner_for_sessions"));
    assert!(complete.contains("managed_surface_observation_binding_stale"));
    assert!(complete.contains("claim_action(request.surface.session_id"));
    assert!(complete.contains("complete_action(&action, request.result)"));

    let preview_take = between(
        &desktop,
        "async fn preview_take_actions(",
        "async fn preview_take_network_fault_controls(",
    );
    assert!(preview_take.contains("take_surface_actions(&surface.identity)"));
    assert!(
        !preview_take.contains(r#"/v1/sessions/{session_id}/actions"#),
        "managed WebViews must not drain the session-wide public queue directly"
    );

    let preview_complete = between(
        &desktop,
        "async fn preview_complete_action(",
        "async fn preview_complete_content_stress(",
    );
    assert!(preview_complete.contains("complete_surface_action("));
    assert!(
        !preview_complete.contains(r#"/v1/sessions/{session_id}/actions/results"#),
        "managed WebViews must not complete public actions outside exact surface authority"
    );

    assert!(surface.contains("/v1/runtime/resources/surfaces/actions/take"));
    assert!(surface.contains("/v1/runtime/resources/surfaces/actions/complete"));
}

#[test]
fn managed_surface_canonical_incarnations_are_daemon_derived() {
    let control = source("../../../crates/control/src/resource_runtime.rs");
    let refs = between(
        &control,
        "fn managed_surface_refs(",
        "fn primary_live_surface(",
    );

    assert!(refs.contains("ProviderIncarnationRef::from"));
    assert!(refs.contains("TargetIncarnationRef::from"));
    assert!(refs.contains("owner_instance_id"));
    assert!(refs.contains("session_id"));
    assert!(refs.contains("identity.surface_kind"));
    assert!(refs.contains("identity.label"));
    assert!(refs.contains("identity.incarnation"));

    let binding = between(
        &control,
        "async fn ensure_managed_surface_observation_binding(",
        "async fn take_surface_actions(",
    );
    assert!(binding.contains("initial_continuity: EventContinuityState::ReconciliationRequired"));
    assert!(binding.contains("release_provider_observation(session_id)"));
    assert!(binding.contains("bind_provider_observation(ProviderObservationBinding"));
}
