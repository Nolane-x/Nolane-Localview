fn main() {
    if localview_macos_ax_provider::run_permission_probe_if_requested() {
        return;
    }

    eprintln!("localview-ax-permission-probe is an internal M02 dispatch-fence helper");
    std::process::exit(64);
}
