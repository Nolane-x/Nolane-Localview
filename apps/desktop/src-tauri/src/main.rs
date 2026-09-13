#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "macos")]
    if localview_macos_ax_provider::run_permission_probe_if_requested() {
        return;
    }

    localview_desktop::run();
}
