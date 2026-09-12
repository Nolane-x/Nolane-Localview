#![cfg(windows)]

use localview_windows_uia_provider::{
    WindowsKeyTransition, WindowsVerifiedKeyEvent, observe_windows_verified_input_context,
    snapshot_windows_keyboard_state, windows_insert_verified_key_events,
};

#[test]
fn windows_backend_surface_is_narrow_and_typed() {
    let _observe: fn(u64, u32, bool) -> _ = observe_windows_verified_input_context;
    let _snapshot: fn() -> _ = snapshot_windows_keyboard_state;
    let _insert: fn(&[WindowsVerifiedKeyEvent]) -> _ = windows_insert_verified_key_events;

    let events = [
        WindowsVerifiedKeyEvent {
            virtual_key: 0x41,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: 0x41,
            transition: WindowsKeyTransition::KeyUp,
        },
    ];
    assert_eq!(events.len(), 2);
}
