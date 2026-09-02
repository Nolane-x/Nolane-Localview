use localview_windows_uia_provider::{
    evaluate_windows_uia_dispatch_context, WindowsUiaDispatchContextBlocker,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements,
};

fn strict_requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: true,
        require_no_modal_blocker: true,
    }
}

fn current_observation() -> WindowsUiaDispatchContextObservation {
    WindowsUiaDispatchContextObservation {
        target_window_handle: 0x7100,
        target_process_id: 71,
        foreground_window_handle: Some(0x7100),
        foreground_process_id: Some(71),
        exact_element_focused: Some(true),
        modal_blocker_window_handle: None,
    }
}

#[test]
fn exact_current_target_focus_and_clear_modal_state_pass() {
    evaluate_windows_uia_dispatch_context(strict_requirements(), &current_observation()).unwrap();
}

#[test]
fn wrong_or_unknown_foreground_fails_closed() {
    let mut wrong_window = current_observation();
    wrong_window.foreground_window_handle = Some(0x7200);
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &wrong_window).unwrap_err(),
        WindowsUiaDispatchContextBlocker::ForegroundWindowMismatch {
            expected: 0x7100,
            actual: 0x7200,
        }
    );

    let mut unknown = current_observation();
    unknown.foreground_window_handle = None;
    unknown.foreground_process_id = None;
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &unknown).unwrap_err(),
        WindowsUiaDispatchContextBlocker::ForegroundUnavailable
    );
}

#[test]
fn same_hwnd_with_wrong_process_is_not_current_target() {
    let mut observation = current_observation();
    observation.foreground_process_id = Some(72);
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &observation).unwrap_err(),
        WindowsUiaDispatchContextBlocker::ForegroundProcessMismatch {
            expected: 71,
            actual: 72,
        }
    );
}

#[test]
fn focus_requirement_rejects_mismatch_and_unknown() {
    let mut mismatch = current_observation();
    mismatch.exact_element_focused = Some(false);
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &mismatch).unwrap_err(),
        WindowsUiaDispatchContextBlocker::ExactElementFocusMismatch
    );

    let mut unknown = current_observation();
    unknown.exact_element_focused = None;
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &unknown).unwrap_err(),
        WindowsUiaDispatchContextBlocker::FocusUnavailable
    );
}

#[test]
fn modal_or_owned_popup_blocker_is_explicit() {
    let mut observation = current_observation();
    observation.modal_blocker_window_handle = Some(0x7300);
    assert_eq!(
        evaluate_windows_uia_dispatch_context(strict_requirements(), &observation).unwrap_err(),
        WindowsUiaDispatchContextBlocker::ModalBlockerPresent {
            window_handle: 0x7300,
        }
    );
}

#[test]
fn weaker_semantic_requirement_does_not_invent_missing_focus_evidence() {
    let requirements = WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: false,
        require_no_modal_blocker: true,
    };
    let mut observation = current_observation();
    observation.exact_element_focused = None;

    evaluate_windows_uia_dispatch_context(requirements, &observation).unwrap();
    assert_eq!(observation.exact_element_focused, None);
}
