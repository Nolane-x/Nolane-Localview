#![forbid(unsafe_code)]

#[test]
fn network_fault_controls_require_exact_live_preview_owner_truth() {
    let lib = include_str!("../src/lib.rs");

    let helper = lib
        .split("fn network_fault_preview_surface(")
        .nth(1)
        .expect("network-fault preview owner helper must exist")
        .split("fn ensure_preview_caller(")
        .next()
        .expect("network-fault preview owner helper must be bounded");

    assert!(helper.contains("workspace_surface::preview_surface_label(session_id)"));
    assert!(helper.contains("DesktopSurfaceKind::PreviewWindow"));
    assert!(helper.contains("registry.current("));
    assert!(helper.contains("current.identity.owner_instance_id != registry.owner_instance_id()"));
    assert!(
        !helper.contains("DesktopSurfaceKind::WorkspaceChild"),
        "Wave 3 network faults must not silently widen to workspace-child authority"
    );
}

#[test]
fn desktop_not_page_authors_network_fault_surface_incarnation() {
    let lib = include_str!("../src/lib.rs");

    let completion = lib
        .split("async fn preview_complete_network_fault_control(")
        .nth(1)
        .expect("network-fault completion command must exist")
        .split("#[tauri::command]\nasync fn preview_action_cancellation(")
        .next()
        .expect("network-fault completion command must be bounded");

    assert!(completion.contains("network_fault_preview_surface("));
    assert!(completion.contains(r#""surface_incarnation".into()"#));
    assert!(completion.contains("surface.identity.incarnation"));

    let bridge = lib
        .split("const PREVIEW_BRIDGE_SCRIPT: &str = r#\"")
        .nth(1)
        .expect("preview bridge script must exist")
        .split("\"#;")
        .next()
        .expect("preview bridge script must be bounded");
    assert!(
        !bridge.contains("surface_incarnation"),
        "page JavaScript must not author desktop surface incarnation"
    );
}

#[test]
fn taking_private_controls_requires_live_preview_registry_authority() {
    let lib = include_str!("../src/lib.rs");

    let take = lib
        .split("async fn preview_take_network_fault_controls(")
        .nth(1)
        .expect("network-fault take command must exist")
        .split("#[tauri::command]\nasync fn preview_complete_network_fault_control(")
        .next()
        .expect("network-fault take command must be bounded");

    assert!(take.contains("DesktopSurfaceRegistry"));
    assert!(take.contains("network_fault_preview_surface(registry.inner()"));
    assert!(
        take.find("network_fault_preview_surface")
            .expect("owner validation")
            < take.find("/network-fault-controls")
                .expect("control-plane fetch"),
        "desktop owner truth must be validated before private controls are fetched"
    );
}

#[test]
fn preview_destruction_invalidates_fault_authority_before_resource_release() {
    let lib = include_str!("../src/lib.rs");

    let reconciler = lib
        .split("fn install_preview_surface_destroyed_reconciler(")
        .nth(1)
        .expect("preview destruction reconciler must exist")
        .split("async fn invalidate_network_fault_preview(")
        .next()
        .expect("preview destruction reconciler must be bounded");

    let invalidate = reconciler
        .find("invalidate_network_fault_preview(identity.session_id, identity.incarnation)")
        .expect("preview destruction must invalidate network-fault authority");
    let release = reconciler
        .find("surface_resource::release_surface(&identity)")
        .expect("preview destruction must release resource authority");

    assert!(
        invalidate < release,
        "network-fault authority must be invalidated before surface resource release"
    );

    let invalidator = lib
        .split("async fn invalidate_network_fault_preview(")
        .nth(1)
        .expect("network-fault invalidator must exist")
        .split("fn preview_registry_error(")
        .next()
        .expect("network-fault invalidator must be bounded");
    assert!(invalidator.contains("/network-faults/invalidate-preview"));
    assert!(invalidator.contains(r#""surface_incarnation""#));
    assert!(invalidator.contains(".bearer_auth(token)"));
}
