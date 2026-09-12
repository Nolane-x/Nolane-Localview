use localview_protocol::DispatchResult;
use localview_windows_observe_runtime::dispatch_result_for_verified_input;
use localview_windows_uia_provider::{
    WindowsInputInsertionClass, WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextObservation,
    WindowsVerifiedInputBoundaryReceipt,
};

fn receipt(
    class: WindowsInputInsertionClass,
    requested: u32,
    inserted: u32,
) -> WindowsVerifiedInputBoundaryReceipt {
    WindowsVerifiedInputBoundaryReceipt {
        dispatch_context: WindowsUiaDispatchContextObservation {
            target_window_handle: 100,
            target_process_id: 7,
            foreground_window_handle: Some(100),
            foreground_process_id: Some(7),
            exact_element_focused: Some(true),
            modal_blocker_window_handle: None,
        },
        keyboard_state: WindowsKeyboardStateSnapshot {
            shift_down: false,
            control_down: false,
            alt_down: false,
            left_windows_down: false,
            right_windows_down: false,
            caps_lock_on: false,
            num_lock_on: false,
            scroll_lock_on: false,
            layout_identity: None,
        },
        requested_event_count: requested,
        inserted_event_count: inserted,
        insertion_class: class,
        raw_error_code: None,
        reconciliation_required: true,
    }
}

#[test]
fn verified_input_counts_map_to_existing_consequential_dispatch_states() {
    assert_eq!(
        dispatch_result_for_verified_input(&receipt(
            WindowsInputInsertionClass::FullyInserted,
            4,
            4,
        )),
        DispatchResult::DispatchedFull,
    );
    assert_eq!(
        dispatch_result_for_verified_input(&receipt(
            WindowsInputInsertionClass::PartialDispatchUnknownOutcome,
            4,
            2,
        )),
        DispatchResult::DispatchedPartial,
    );
    assert_eq!(
        dispatch_result_for_verified_input(&receipt(
            WindowsInputInsertionClass::ZeroInsertedBlocked,
            4,
            0,
        )),
        DispatchResult::NotDispatched,
    );
}
