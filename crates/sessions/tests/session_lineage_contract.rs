use localview_protocol::{Endpoint, ProjectIdentity, ServerKind};
use localview_sessions::session_lineage;

fn endpoint(scheme: &str, host: &str, port: u16) -> Endpoint {
    Endpoint {
        scheme: scheme.to_owned(),
        host: host.to_owned(),
        port,
    }
}

fn project(root: Option<&str>, cwd: Option<&str>, key: &str) -> ProjectIdentity {
    ProjectIdentity {
        key: key.to_owned(),
        display_name: "app".into(),
        cwd: cwd.map(str::to_owned),
        git_root: root.map(str::to_owned),
        pid: Some(11),
        command: Some("vite --port 5173".into()),
    }
}

#[test]
fn project_lineage_ignores_port_scheme_and_legacy_project_key() {
    let first_project = project(Some("/work/app"), Some("/work/app"), "legacy-a");
    let second_project = project(Some("/work/app/"), Some("/work/app"), "legacy-b");

    let first = session_lineage(
        &first_project,
        &endpoint("http", "127.0.0.1", 5173),
        ServerKind::FrontendDevServer,
    )
    .expect("project lineage should be valid");
    let moved = session_lineage(
        &second_project,
        &endpoint("https", "127.0.0.1", 6200),
        ServerKind::FrontendDevServer,
    )
    .expect("project lineage should be valid");

    assert_eq!(first, moved);
}

#[test]
fn same_project_with_different_server_kind_has_different_lineage() {
    let project = project(Some("/work/app"), Some("/work/app"), "legacy");
    let frontend = session_lineage(
        &project,
        &endpoint("http", "127.0.0.1", 5173),
        ServerKind::FrontendDevServer,
    )
    .unwrap();
    let storybook = session_lineage(
        &project,
        &endpoint("http", "127.0.0.1", 5173),
        ServerKind::Storybook,
    )
    .unwrap();

    assert_ne!(frontend, storybook);
}

#[test]
fn projectless_endpoint_fallback_is_exact() {
    let project = project(None, None, "");
    let first = session_lineage(
        &project,
        &endpoint("http", "127.0.0.1", 5173),
        ServerKind::UnknownHttp,
    )
    .unwrap();
    let moved_port = session_lineage(
        &project,
        &endpoint("http", "127.0.0.1", 5174),
        ServerKind::UnknownHttp,
    )
    .unwrap();
    let moved_scheme = session_lineage(
        &project,
        &endpoint("https", "127.0.0.1", 5173),
        ServerKind::UnknownHttp,
    )
    .unwrap();

    assert_ne!(first, moved_port);
    assert_ne!(first, moved_scheme);
}

#[test]
fn unresolved_parent_traversal_is_rejected() {
    let project = project(Some("/work/../app"), Some("/work/../app"), "legacy");
    let result = session_lineage(
        &project,
        &endpoint("http", "127.0.0.1", 5173),
        ServerKind::FrontendDevServer,
    );

    assert!(result.is_err());
}
