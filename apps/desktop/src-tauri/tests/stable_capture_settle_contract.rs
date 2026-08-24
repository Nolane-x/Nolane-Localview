#[test]
fn viewport_capture_waits_for_bounded_stable_settle_before_native_pixels() {
    let module = include_str!("../src/visual_capture.rs");

    assert!(module.contains("StableCapturePolicy::default()"));
    assert!(module.contains("/capture-settle"));
    assert!(module.contains("policy.timeout_ms"));
    assert!(module.contains("decision.retry_after_ms.clamp(25, 100)"));
    assert!(module.contains("stable capture settle timed out"));
    assert!(module.contains("preflight_managed_surface(&app, session_id)?"));

    let preflight = module
        .find("preflight_managed_surface(&app, session_id)?")
        .expect("managed surface must be preflighted");
    let settle = module
        .find("wait_for_capture_settle(session_id).await?")
        .expect("capture must wait for settle");
    let capture = module
        .find("capture_managed_surface(&app, session_id, viewport, revision).await")
        .expect("native capture call must remain explicit");

    assert!(
        preflight < settle,
        "surface preflight must happen before settle polling"
    );
    assert!(
        settle < capture,
        "native pixels must never be captured before settle succeeds"
    );
    assert!(module.contains("Duration::from_millis(policy.timeout_ms)"));
    assert!(module.contains("tokio::time::timeout"));
    assert!(!module.contains("settle timeout fallback"));
}

#[test]
fn managed_route_is_read_and_revalidated_after_settle() {
    let module = include_str!("../src/visual_capture.rs");
    let settle = module
        .find("wait_for_capture_settle(session_id).await?")
        .expect("capture settle call");
    let capture_function = module
        .find("async fn capture_managed_surface(")
        .expect("native capture coordinator");
    let route_read = module[capture_function..]
        .find("window.url()")
        .expect("managed route must be read inside native capture coordinator")
        + capture_function;
    let loopback_check = module[route_read..]
        .find("workspace_navigation_allowed(&route_url)")
        .expect("route must be revalidated after settle")
        + route_read;

    assert!(settle < route_read);
    assert!(route_read < loopback_check);
}
