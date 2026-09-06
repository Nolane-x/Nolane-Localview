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

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{DispatchResult, TransportResult};
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
        WindowsUiaPatternDispatchRequest, WindowsUiaPatternSupport, WindowsUiaSnapshotRequest,
        WindowsUiaWorker, WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        Win32::{
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                BS_AUTOCHECKBOX, CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW,
                MSG, PM_REMOVE, PeekMessageW, SW_SHOW, ShowWindow, TranslateMessage, WINDOW_STYLE,
                WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn dispatches_toggle_on_the_exact_retained_checkbox() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-toggle-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView Toggle Dispatch Smoke"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        460,
                        260,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create Toggle fixture parent")
                };
                let checkbox_style =
                    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | BS_AUTOCHECKBOX.0);
                unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView Toggle"),
                        checkbox_style,
                        32,
                        48,
                        220,
                        44,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create Toggle fixture checkbox");
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish Toggle fixture HWND");

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
                    DestroyWindow(window).expect("destroy Toggle fixture");
                }
            })
            .expect("spawn responsive Toggle fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive Toggle fixture HWND");
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
            .expect("attach exact Toggle fixture target");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-toggle-smoke:1".into(),
                    surface_scope: "fixture:win32-checkbox".into(),
                },
            )
            .expect("publish retained semantic snapshot before Toggle dispatch");
        let checkbox = snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some("LocalView Toggle"))
            .expect("real Win32 checkbox must be present in the semantic snapshot");
        assert_eq!(
            WindowsUiaActionCapabilities::from_node(checkbox)
                .support_for(WindowsUiaPattern::Toggle),
            WindowsUiaPatternSupport::Supported,
            "fixture checkbox must publish live Toggle support before dispatch"
        );

        let dispatch_attempt_ref = Uuid::new_v4();
        let action_id = Uuid::new_v4();
        let request = WindowsUiaPatternDispatchRequest {
            dispatch_attempt_ref,
            action_id,
            preparation_journal_sequence: 1,
            preparation_receipt_ref: "prepare:windows-uia-toggle-smoke:1".into(),
            snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            element_ref: checkbox.element_ref.clone(),
            required_pattern: WindowsUiaPattern::Toggle,
            context_requirements: WindowsUiaDispatchContextRequirements {
                require_foreground_target: false,
                require_exact_element_focus: false,
                require_no_modal_blocker: true,
            },
        };

        let receipt = worker
            .dispatch_pattern(&attachment, request)
            .expect("Toggle the exact retained live UIA checkbox on its owning MTA worker");

        assert_eq!(receipt.dispatch_attempt_ref, dispatch_attempt_ref);
        assert_eq!(receipt.action_id, action_id);
        assert_eq!(receipt.required_pattern, WindowsUiaPattern::Toggle);
        assert_eq!(receipt.element_ref, checkbox.element_ref);
        assert_eq!(
            receipt.transport_result,
            TransportResult::DeliveredToExecutor
        );
        assert_eq!(receipt.dispatch_result, DispatchResult::DispatchedFull);

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join Toggle fixture UI thread");
    }
}
