use std::{ffi::c_void, mem::size_of};

use windows::Win32::{
    Foundation::{GetLastError, HWND},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD,
            KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CAPITAL,
            VK_CONTROL, VK_LWIN, VK_MENU, VK_NUMLOCK, VK_RWIN, VK_SCROLL, VK_SHIFT,
        },
        WindowsAndMessaging::{
            GetForegroundWindow, GetLastActivePopup, GetWindowThreadProcessId, IsWindowVisible,
        },
    },
};

use crate::{
    WindowsInputInsertRawResult, WindowsKeyTransition, WindowsKeyboardStateSnapshot,
    WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError,
    WindowsVerifiedKeyEvent,
};

fn hwnd_from_u64(value: u64) -> HWND {
    HWND(value as usize as *mut c_void)
}

fn hwnd_to_u64(value: HWND) -> Option<u64> {
    (!value.0.is_null()).then_some(value.0 as usize as u64)
}

fn key_is_down(virtual_key: VIRTUAL_KEY) -> bool {
    let state = unsafe {
        // SAFETY: GetAsyncKeyState accepts a virtual-key identifier and has no
        // borrowed-pointer lifetime. We use only the high-order current-state bit.
        GetAsyncKeyState(virtual_key.0 as i32)
    };
    (state as u16 & 0x8000) != 0
}

fn key_is_toggled(virtual_key: VIRTUAL_KEY) -> bool {
    let state = unsafe {
        // SAFETY: GetKeyState accepts a virtual-key identifier and has no
        // borrowed-pointer lifetime. We use only the low-order toggle bit.
        GetKeyState(virtual_key.0 as i32)
    };
    (state as u16 & 0x0001) != 0
}

/// Read the volatile Win32 window context immediately before a verified input
/// attempt. Exact UIA element focus is deliberately left as `None`: callers that
/// require exact element focus must enrich/recheck it in their owning UIA
/// apartment, and the shared context evaluator will fail closed if they request
/// exact focus without supplying it.
pub fn observe_windows_verified_input_context(
    target_window_handle: u64,
    target_process_id: u32,
    require_no_modal_blocker: bool,
) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError> {
    if target_window_handle == 0 || target_process_id == 0 {
        return Err(WindowsVerifiedInputBoundaryError::ContextObservationFailed);
    }

    let foreground = unsafe {
        // SAFETY: read-only Win32 query with no borrowed pointers.
        GetForegroundWindow()
    };
    let foreground_window_handle = hwnd_to_u64(foreground);
    let foreground_process_id = foreground_window_handle.and_then(|_| {
        let mut process_id = 0_u32;
        let thread_id = unsafe {
            // SAFETY: process_id is valid writable storage and foreground was
            // returned by GetForegroundWindow immediately above.
            GetWindowThreadProcessId(foreground, Some(&mut process_id))
        };
        (thread_id != 0 && process_id != 0).then_some(process_id)
    });

    let target = hwnd_from_u64(target_window_handle);
    let modal_blocker_window_handle = if require_no_modal_blocker {
        let popup = unsafe {
            // SAFETY: target is an opaque HWND value supplied from an already
            // authority-bound target selection. This query does not retain it.
            GetLastActivePopup(target)
        };
        match hwnd_to_u64(popup) {
            Some(handle)
                if handle != target_window_handle
                    && unsafe { IsWindowVisible(popup) }.as_bool() =>
            {
                Some(handle)
            }
            _ => None,
        }
    } else {
        None
    };

    Ok(WindowsUiaDispatchContextObservation {
        target_window_handle,
        target_process_id,
        foreground_window_handle,
        foreground_process_id,
        exact_element_focused: None,
        modal_blocker_window_handle,
    })
}

/// Snapshot only correctness-relevant keyboard state. This never modifies or
/// normalizes human-held keys.
pub fn snapshot_windows_keyboard_state()
-> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError> {
    let layout = unsafe {
        // SAFETY: thread id 0 requests the active input locale for the current
        // thread; the returned handle is used only as opaque evidence metadata.
        GetKeyboardLayout(0)
    };
    let layout_identity = (!layout.0.is_null()).then(|| format!("0x{:x}", layout.0 as usize));

    Ok(WindowsKeyboardStateSnapshot {
        shift_down: key_is_down(VK_SHIFT),
        control_down: key_is_down(VK_CONTROL),
        alt_down: key_is_down(VK_MENU),
        left_windows_down: key_is_down(VK_LWIN),
        right_windows_down: key_is_down(VK_RWIN),
        caps_lock_on: key_is_toggled(VK_CAPITAL),
        num_lock_on: key_is_toggled(VK_NUMLOCK),
        scroll_lock_on: key_is_toggled(VK_SCROLL),
        layout_identity,
    })
}

fn to_input(event: WindowsVerifiedKeyEvent) -> INPUT {
    let flags = match event.transition {
        WindowsKeyTransition::KeyDown => KEYBD_EVENT_FLAGS::default(),
        WindowsKeyTransition::KeyUp => KEYEVENTF_KEYUP,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(event.virtual_key),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Perform one platform insertion attempt and report the exact raw count. A
/// zero or partial count is not interpreted here as a particular policy cause.
pub fn windows_insert_verified_key_events(
    events: &[WindowsVerifiedKeyEvent],
) -> WindowsInputInsertRawResult {
    let inputs: Vec<INPUT> = events.iter().copied().map(to_input).collect();
    let requested_event_count = inputs.len() as u32;
    let input_size = i32::try_from(size_of::<INPUT>()).expect("INPUT size fits i32");
    let inserted_event_count = unsafe {
        // SAFETY: inputs is a live contiguous slice for the duration of this
        // synchronous call and input_size is exactly size_of::<INPUT>().
        SendInput(&inputs, input_size)
    };
    let raw_error_code = (inserted_event_count != requested_event_count).then(|| unsafe {
        // SAFETY: read-only thread-local last-error query immediately after the
        // failed/partial API call. A zero value is filtered below.
        GetLastError().0
    });

    WindowsInputInsertRawResult {
        requested_event_count,
        inserted_event_count,
        raw_error_code: raw_error_code.filter(|code| *code != 0),
    }
}
