use localview_windows_uia_provider::{
    WindowsInputInsertRawResult, WindowsKeyTransition, WindowsKeyboardStateSnapshot,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements,
    WindowsVerifiedInputBoundaryError, WindowsVerifiedInputEnvironment, WindowsVerifiedKeyEvent,
    WindowsVerifiedKeyboardBatch, execute_windows_verified_input_boundary,
};

#[derive(Debug)]
struct FakeEnvironment {
    context: WindowsUiaDispatchContextObservation,
    keyboard: WindowsKeyboardStateSnapshot,
    inserted: u32,
    calls: Vec<&'static str>,
}

impl WindowsVerifiedInputEnvironment for FakeEnvironment {
    fn observe_dispatch_context(
        &mut self,
    ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError> {
        self.calls.push("context");
        Ok(self.context.clone())
    }

    fn snapshot_keyboard_state(
        &mut self,
    ) -> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError> {
        self.calls.push("keyboard");
        Ok(self.keyboard.clone())
    }

    fn insert_events(&mut self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult {
        self.calls.push("insert");
        WindowsInputInsertRawResult {
            requested_event_count: events.len() as u32,
            inserted_event_count: self.inserted,
            raw_error_code: None,
        }
    }
}

fn batch() -> WindowsVerifiedKeyboardBatch {
    WindowsVerifiedKeyboardBatch::new(vec![
        WindowsVerifiedKeyEvent {
            virtual_key: 0x41,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: 0x41,
            transition: WindowsKeyTransition::KeyUp,
        },
    ])
    .unwrap()
}

fn requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: false,
        require_no_modal_blocker: true,
    }
}

fn context(foreground: u64) -> WindowsUiaDispatchContextObservation {
    WindowsUiaDispatchContextObservation {
        target_window_handle: 100,
        target_process_id: 7,
        foreground_window_handle: Some(foreground),
        foreground_process_id: Some(if foreground == 100 { 7 } else { 8 }),
        exact_element_focused: None,
        modal_blocker_window_handle: None,
    }
}

fn keyboard() -> WindowsKeyboardStateSnapshot {
    WindowsKeyboardStateSnapshot {
        shift_down: false,
        control_down: false,
        alt_down: false,
        left_windows_down: false,
        right_windows_down: false,
        caps_lock_on: false,
        num_lock_on: false,
        scroll_lock_on: false,
        layout_identity: None,
    }
}

#[test]
fn foreground_mismatch_blocks_before_keyboard_snapshot_or_insertion() {
    let mut env = FakeEnvironment {
        context: context(200),
        keyboard: keyboard(),
        inserted: 2,
        calls: Vec::new(),
    };

    let error = execute_windows_verified_input_boundary(requirements(), &batch(), &mut env)
        .expect_err("stolen foreground must block final input boundary");

    assert!(matches!(
        error,
        WindowsVerifiedInputBoundaryError::ContextBlocked(_)
    ));
    assert_eq!(env.calls, vec!["context"]);
}

#[test]
fn conflicting_modifier_blocks_before_platform_insertion() {
    let mut held = keyboard();
    held.shift_down = true;
    let mut env = FakeEnvironment {
        context: context(100),
        keyboard: held,
        inserted: 2,
        calls: Vec::new(),
    };

    let error = execute_windows_verified_input_boundary(requirements(), &batch(), &mut env)
        .expect_err("held human modifier must block platform insertion");

    assert_eq!(error, WindowsVerifiedInputBoundaryError::InputStateConflict,);
    assert_eq!(env.calls, vec!["context", "keyboard"]);
}

#[test]
fn successful_boundary_orders_final_fences_before_single_insertion() {
    let mut env = FakeEnvironment {
        context: context(100),
        keyboard: keyboard(),
        inserted: 2,
        calls: Vec::new(),
    };

    let receipt = execute_windows_verified_input_boundary(requirements(), &batch(), &mut env)
        .expect("neutral final state should reach one insertion attempt");

    assert_eq!(env.calls, vec!["context", "keyboard", "insert"]);
    assert_eq!(receipt.requested_event_count, 2);
    assert_eq!(receipt.inserted_event_count, 2);
    assert!(receipt.reconciliation_required);
}
