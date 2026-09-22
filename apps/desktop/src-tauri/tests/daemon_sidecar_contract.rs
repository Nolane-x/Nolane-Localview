use std::fs;

fn source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file must be readable")
}

#[test]
fn desktop_bundles_and_bootstraps_daemon_without_frontend_shell_authority() {
    let lib = source("src/lib.rs");
    let runtime = source("src/daemon_sidecar.rs");
    let config = source("tauri.conf.json");
    let cargo = source("Cargo.toml");
    let capability = source("capabilities/default.json");
    let package = source("../package.json");
    let prepare = source("../scripts/prepare-sidecar.mjs");
    let build = source("build.rs");

    assert!(cargo.contains(r#"tauri-plugin-shell = "2.3.6""#));
    assert!(lib.contains(".plugin(tauri_plugin_shell::init())"));
    assert!(lib.contains("daemon_sidecar::ensure_daemon(app.handle().clone())"));
    assert!(lib.contains("daemon_sidecar::stop_owned_daemon(app)"));

    assert!(config.contains(r#""externalBin": ["binaries/localview-daemon"]"#));
    assert!(config.contains("npm run prepare:sidecar:dev && npm run dev"));
    assert!(config.contains("npm run build && npm run prepare:sidecar"));

    assert!(package.contains(r#""prepare:sidecar""#));
    assert!(package.contains(r#""prepare:sidecar:dev""#));
    assert!(prepare.contains("cargoArgs = ['build', '-p', 'localview-daemon']"));
    assert!(prepare.contains("rustc"));
    assert!(prepare.contains("--print"));
    assert!(prepare.contains("host-tuple"));
    assert!(build.contains("ensure_sidecar_manifest_placeholder"));
    assert!(build.contains("localview-daemon-{target}{extension}"));
    assert!(build.contains("if !sidecar.exists()"));

    assert!(runtime.contains("http://127.0.0.1:45454/health"));
    assert!(runtime.contains("Policy::none()"));
    assert!(runtime.contains(".no_proxy()"));
    assert!(runtime.contains(r#".sidecar("localview-daemon")"#));
    assert!(runtime.contains("receipt.version != env!(\"CARGO_PKG_VERSION\")"));
    assert!(runtime.contains("STARTUP_RETRIES"));
    assert!(runtime.contains("child.kill()"));

    assert!(
        !capability.contains("shell:"),
        "the dashboard WebView must not receive generic shell/sidecar authority"
    );
}
