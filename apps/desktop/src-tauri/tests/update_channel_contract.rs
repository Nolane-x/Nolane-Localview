#![forbid(unsafe_code)]

use std::fs;

fn source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file must be readable")
}

#[test]
fn update_channel_is_check_only_and_fail_closed_without_signature_authority() {
    let updater = source("src/update_channel.rs");
    assert!(updater.contains("option_env!(\"LOCALVIEW_UPDATE_MANIFEST_URL\")"));
    assert!(updater.contains(".redirect(Policy::none())"));
    assert!(updater.contains("MAX_MANIFEST_BYTES"));
    assert!(updater.contains("update manifest content type is missing"));
    assert!(updater.contains("update manifest contains duplicate OS/architecture artifacts"));
    assert!(updater.contains("install_authorized: false"));
    assert!(updater.contains("same_origin(manifest_url, &artifact_url)"));
    assert!(updater.contains("update artifact must remain on the pinned manifest origin"));
    assert!(updater.contains("fetch_manifest(&manifest_url).await?"));
    assert_eq!(
        updater.matches(".get(url.clone())").count(),
        1,
        "R16 may fetch only the pinned manifest URL"
    );
    for forbidden in [
        "std::fs::write",
        "tokio::fs::write",
        "std::fs::File::create",
        "OpenOptions::new",
        "std::process::Command",
        "tokio::process::Command",
        "tauri_plugin_updater",
        "fetch_artifact",
        "download_artifact",
        "install_artifact",
        "apply_update",
    ] {
        assert!(
            !updater.contains(forbidden),
            "check-only updater unexpectedly contains forbidden artifact/apply authority: {forbidden}"
        );
    }
}
