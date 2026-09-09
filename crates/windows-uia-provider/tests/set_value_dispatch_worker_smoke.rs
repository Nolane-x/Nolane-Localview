#[cfg(windows)]
mod windows_smoke {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use localview_live_bridge::CanonicalActionOperation;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{DispatchResult, TransportResult};
    use localview_windows_uia_provider::{
        SetValueMode, SetValuePayloadRef, WindowsUiaActionCapabilities,
        WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
        WindowsUiaPatternSupport, WindowsUiaSetValueDispatchRequest,
        WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        Win32::{
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, GetWindowTextW,
                MSG, PM_REMOVE, PeekMessageW, SW_SHOW, ShowWindow, TranslateMessage,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn set_value_dispatches_once_on_exact_edit_and_returns_metadata_only_receipt() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-set-value-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        w!("LocalView SetValue Before"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        520,
                        180,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create deterministic editable Win32 SetValue fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish SetValue smoke HWND");

                let mut message = MSG::default();
                while !ui_stop.load(Ordering::Acquire) {
                    while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                        unsafe {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    thread::sleep(Duration::from_millis(2));
                }

                unsafe {
                    DestroyWindow(window).expect("destroy Win32 SetValue fixture");
                }
            })
            .expect("spawn responsive Win32 SetValue UI thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive live SetValue smoke HWND");
        let worker = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 32,
                max_depth: 4,
                max_properties: 256,
            },
            command_timeout: Duration::from_secs(5),
        })
        .expect("spawn dedicated Windows UIA MTA worker");

        let process_id = unsafe { GetCurrentProcessId() };
        let attachment = worker
            .attach(UserSelectedWindowTarget {
                native_window_handle: window_handle,
                expected_process_id: process_id,
                selection_nonce: Uuid::new_v4(),
            })
            .expect("attach exact user-selected editable Win32 target");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-set-value-smoke:1".into(),
                    surface_scope: "fixture:win32-edit".into(),
                },
            )
            .expect("publish retained semantic snapshot before SetValue");
        let fixture_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                WindowsUiaActionCapabilities::from_node(node)
                    .support_for(WindowsUiaPattern::Value)
                    == WindowsUiaPatternSupport::Supported
            })
            .expect("editable Win32 fixture must publish live ValuePattern support");

        let payload_ref = SetValuePayloadRef(Uuid::new_v4());
        let replacement = "LocalView SetValue After";
        let request = WindowsUiaSetValueDispatchRequest::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            "prepare:windows-uia-set-value-smoke:1".into(),
            snapshot.snapshot_cut_ref().into(),
            attachment.provider_incarnation_ref().clone(),
            attachment.target_incarnation_ref().clone(),
            fixture_node.element_ref.clone(),
            WindowsUiaDispatchContextRequirements {
                require_foreground_target: false,
                require_exact_element_focus: false,
                require_no_modal_blocker: true,
            },
            payload_ref,
            SetValueMode::ReplaceValue,
            replacement.as_bytes().to_vec(),
        )
        .expect("construct bounded move-only SetValue worker request");

        let receipt = worker
            .dispatch_set_value(&attachment, request)
            .expect("SetValue exact retained editable UIA element once");

        let mut buffer = [0u16; 128];
        let copied = unsafe {
            GetWindowTextW(
                windows::Win32::Foundation::HWND(window_handle as isize as *mut _),
                &mut buffer,
            )
        };
        assert!(copied > 0, "fixture text must remain readable after SetValue");
        let observed = String::from_utf16(&buffer[..copied as usize])
            .expect("Win32 fixture value is valid UTF-16");
        assert_eq!(observed, replacement);

        assert_eq!(receipt.required_pattern, WindowsUiaPattern::Value);
        assert_eq!(receipt.dispatch_operation, CanonicalActionOperation::SetValue);
        assert_eq!(receipt.payload_ref, payload_ref);
        assert_eq!(receipt.mode, SetValueMode::ReplaceValue);
        assert_eq!(receipt.transport_result, TransportResult::DeliveredToExecutor);
        assert_eq!(receipt.dispatch_result, DispatchResult::DispatchedFull);
        assert!(!format!("{receipt:?}").contains(replacement));

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join SetValue smoke UI thread");
    }
}
