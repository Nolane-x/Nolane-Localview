#![cfg(windows)]

use localview_windows_uia_provider::{
    observe_windows_verified_input_context, snapshot_windows_keyboard_state,
};

#[test]
fn windows_read_only_backend_observation_surface_is_narrow_and_typed() {
    let _observe: fn(u64, u32, bool) -> _ = observe_windows_verified_input_context;
    let _snapshot: fn() -> _ = snapshot_windows_keyboard_state;
}
