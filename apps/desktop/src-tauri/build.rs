fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "dashboard_state",
                "pause_runtime",
                "resume_runtime",
                "open_preview",
                "preview_ingest",
                "preview_take_actions",
                "preview_complete_action",
            ]),
        ),
    )
    .expect("failed to build LocalView Tauri manifest");
}
