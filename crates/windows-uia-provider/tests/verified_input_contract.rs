use localview_windows_uia_provider::{
    WindowsInputDispatchBlocker, WindowsInputInsertionClass, WindowsKeyTransition,
    WindowsKeyboardStateSnapshot, WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch,
    WindowsVerifiedKeyboardBatchError, classify_windows_input_insertion,
    evaluate_windows_keyboard_state,
};

fn key(virtual_key: u16, transition: WindowsKeyTransition) -> WindowsVerifiedKeyEvent {
    WindowsVerifiedKeyEvent {
        virtual_key,
        transition,
    }
}

fn neutral_state() -> WindowsKeyboardStateSnapshot {
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
fn verified_keyboard_batch_is_bounded_and_order_preserving() {
    assert_eq!(
        WindowsVerifiedKeyboardBatch::new(vec![]),
        Err(WindowsVerifiedKeyboardBatchError::EmptyBatch),
    );

    assert_eq!(
        WindowsVerifiedKeyboardBatch::new(vec![key(0, WindowsKeyTransition::KeyDown)]),
        Err(WindowsVerifiedKeyboardBatchError::InvalidVirtualKey { index: 0 }),
    );

    let too_many = (0..33)
        .map(|_| key(0x41, WindowsKeyTransition::KeyDown))
        .collect();
    assert_eq!(
        WindowsVerifiedKeyboardBatch::new(too_many),
        Err(WindowsVerifiedKeyboardBatchError::TooManyEvents {
            actual: 33,
            maximum: 32,
        }),
    );

    let expected = vec![
        key(0x11, WindowsKeyTransition::KeyDown),
        key(0x53, WindowsKeyTransition::KeyDown),
        key(0x53, WindowsKeyTransition::KeyUp),
        key(0x11, WindowsKeyTransition::KeyUp),
    ];
    let batch = WindowsVerifiedKeyboardBatch::new(expected.clone()).unwrap();
    assert_eq!(batch.events(), expected.as_slice());
}

#[test]
fn held_human_modifiers_fail_closed_without_normalization() {
    assert_eq!(evaluate_windows_keyboard_state(&neutral_state()), Ok(()));

    for mut state in [
        WindowsKeyboardStateSnapshot {
            shift_down: true,
            ..neutral_state()
        },
        WindowsKeyboardStateSnapshot {
            control_down: true,
            ..neutral_state()
        },
        WindowsKeyboardStateSnapshot {
            alt_down: true,
            ..neutral_state()
        },
        WindowsKeyboardStateSnapshot {
            left_windows_down: true,
            ..neutral_state()
        },
        WindowsKeyboardStateSnapshot {
            right_windows_down: true,
            ..neutral_state()
        },
    ] {
        assert_eq!(
            evaluate_windows_keyboard_state(&state),
            Err(WindowsInputDispatchBlocker::InputStateConflict),
        );
        state.shift_down = false;
    }
}

#[test]
fn insertion_count_classification_preserves_partial_and_unknown_semantics() {
    assert_eq!(
        classify_windows_input_insertion(4, 4).unwrap(),
        WindowsInputInsertionClass::FullyInserted,
    );
    assert_eq!(
        classify_windows_input_insertion(4, 2).unwrap(),
        WindowsInputInsertionClass::PartialDispatchUnknownOutcome,
    );
    assert_eq!(
        classify_windows_input_insertion(4, 0).unwrap(),
        WindowsInputInsertionClass::ZeroInsertedBlocked,
    );
    assert_eq!(
        classify_windows_input_insertion(4, 5),
        Err(WindowsInputDispatchBlocker::BackendResultInvalid {
            requested: 4,
            inserted: 5,
        }),
    );
}
