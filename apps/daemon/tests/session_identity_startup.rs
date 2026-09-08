const MAIN: &str = include_str!("../src/main.rs");

fn position(needle: &str) -> usize {
    MAIN.find(needle)
        .unwrap_or_else(|| panic!("daemon startup source is missing required contract: {needle}"))
}

#[test]
fn identity_resolver_is_opened_before_session_manager_and_discovery() {
    let state_root = position("let state_root = state_dir()?");
    let resolver = position("SessionIdentityResolver::open_file");
    let sessions = position("SessionManager::with_identity_resolver");
    let discovery = position("DiscoveryEngine::new");

    assert!(
        state_root < resolver && resolver < sessions && sessions < discovery,
        "durable identity must be rooted and opened before SessionManager and discovery begin"
    );
}

#[test]
fn degraded_identity_registry_warns_but_does_not_abort_daemon_startup() {
    assert!(MAIN.contains("SessionIdentityHealth::VolatileDegraded"));
    assert!(MAIN.contains("session identity continuity degraded; using volatile session identity"));

    let resolver = position("SessionIdentityResolver::open_file");
    let statement_end = MAIN[resolver..]
        .find(';')
        .map(|offset| resolver + offset)
        .expect("resolver open statement should terminate");
    let resolver_statement = &MAIN[resolver..=statement_end];
    assert!(
        !resolver_statement.contains('?'),
        "identity registry degradation must be handled by the resolver rather than aborting startup"
    );
}

#[test]
fn identity_registry_reuses_existing_daemon_state_root_without_restoring_runtime_authority() {
    assert!(MAIN.contains("state_root.join(SESSION_IDENTITY_REGISTRY_FILE)"));
    assert!(MAIN.contains("open_boot_consequential_recovery(&state_root)"));
    assert!(MAIN.contains("state_root.join(\"chromium-runtime\")"));
    assert!(MAIN.contains("load_or_create_token(&state_root)"));

    assert!(
        !MAIN.contains("restore_preview_visible")
            && !MAIN.contains("restore_resource_lease")
            && !MAIN.contains("restore_provider_incarnation")
            && !MAIN.contains("restore_action_authority"),
        "daemon restart may reuse SessionId only; process-local runtime authority must remain fresh"
    );
}
