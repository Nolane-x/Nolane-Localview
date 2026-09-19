fn main() {
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
