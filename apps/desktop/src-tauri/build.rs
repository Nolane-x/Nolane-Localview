use std::{env, fs, path::PathBuf};

fn ensure_sidecar_manifest_placeholder() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be available"),
    );
    let target = env::var("TARGET").expect("TARGET must be available");
    let extension = if target.contains("windows") { ".exe" } else { "" };
    let binary_dir = manifest_dir.join("binaries");
    fs::create_dir_all(&binary_dir).expect("failed to create LocalView sidecar manifest directory");
    let sidecar = binary_dir.join(format!("localview-daemon-{target}{extension}"));

    // Tauri validates externalBin during ordinary cargo check/test. The real daemon is
    // built and atomically copied over this ignored placeholder by the beforeDev/build hook.
    if !sidecar.exists() {
        fs::write(&sidecar, []).expect("failed to create LocalView sidecar manifest placeholder");
    }

    println!("cargo:rerun-if-env-changed=TARGET");
}

fn main() {
    ensure_sidecar_manifest_placeholder();

    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "dashboard_state",
                "live_session_state",
                "action_correlation",
                "pause_runtime",
                "resume_runtime",
                "open_preview",
                "preview_ingest",
                "preview_take_actions",
                "preview_take_network_fault_controls",
                "preview_complete_network_fault_control",
                "preview_complete_action",
            ]),
        ),
    )
    .expect("failed to build LocalView Tauri manifest");
}
