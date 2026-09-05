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
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
                PeekMessageW, SW_SHOW, ShowWindow, TranslateMessage, WS_OVERLAPPEDWINDOW,
                WS_VISIBLE,
            },
        },
        core::w,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn dispatches_invoke_on_the_exact_retained_element_and_returns_only_dispatch_evidence() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-dispatch-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView UIA Dispatch Smoke"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        420,
                        180,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create deterministic Win32 Invoke fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish dispatch smoke HWND");

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
                    DestroyWindow(window).expect("destroy Win32 Invoke fixture");
                }
            })
            .expect("spawn responsive Win32 dispatch smoke UI thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive live dispatch smoke HWND");
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
            .expect("attach exact user-selected Win32 target");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-dispatch-smoke:1".into(),
                    surface_scope: "fixture:win32-invoke-button".into(),
                },
            )
            .expect("publish retained semantic snapshot before dispatch");
        let fixture_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.name
                    .as_deref()
                    .is_some_and(|name| name.contains("LocalView UIA Dispatch Smoke"))
            })
            .expect("real Win32 button must be present in the semantic snapshot");
        assert_eq!(
            WindowsUiaActionCapabilities::from_node(fixture_node)
                .support_for(WindowsUiaPattern::Invoke),
            WindowsUiaPatternSupport::Supported,
            "fixture must publish live Invoke support before dispatch"
        );

        let dispatch_attempt_ref = Uuid::new_v4();
        let action_id = Uuid::new_v4();
        let requirements = WindowsUiaDispatchContextRequirements {
            require_foreground_target: false,
            require_exact_element_focus: false,
            require_no_modal_blocker: true,
        };
        let request = WindowsUiaPatternDispatchRequest {
            dispatch_attempt_ref,
            action_id,
            preparation_journal_sequence: 1,
            preparation_receipt_ref: "prepare:windows-uia-dispatch-smoke:1".into(),
            snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            element_ref: fixture_node.element_ref.clone(),
            required_pattern: WindowsUiaPattern::Invoke,
            context_requirements: requirements,
        };

        let receipt = worker
            .dispatch_pattern(&attachment, request.clone())
            .expect("Invoke the exact retained live UIA pattern on its owning MTA worker");

        assert_eq!(receipt.dispatch_attempt_ref, dispatch_attempt_ref);
        assert_eq!(receipt.action_id, action_id);
        assert_eq!(receipt.preparation_journal_sequence, 1);
        assert_eq!(
            receipt.preparation_receipt_ref,
            "prepare:windows-uia-dispatch-smoke:1"
        );
        assert_eq!(receipt.snapshot_cut_ref, snapshot.snapshot_cut_ref());
        assert_eq!(
            receipt.provider_incarnation_ref,
            *attachment.provider_incarnation_ref()
        );
        assert_eq!(
            receipt.target_incarnation_ref,
            *attachment.target_incarnation_ref()
        );
        assert_eq!(receipt.element_ref, fixture_node.element_ref);
        assert_eq!(receipt.required_pattern, WindowsUiaPattern::Invoke);
        assert_eq!(receipt.context_requirements, requirements);
        assert_eq!(receipt.final_context.target_window_handle, window_handle);
        assert_eq!(receipt.final_context.target_process_id, process_id);
        assert_eq!(
            receipt.transport_result,
            TransportResult::DeliveredToExecutor
        );
        assert_eq!(receipt.dispatch_result, DispatchResult::DispatchedFull);

        assert!(
            !WindowsUiaWorker::capabilities().write_actions
                && !WindowsUiaWorker::capabilities().input_dispatch,
            "a proven provider call must not silently advertise end-to-end write authority"
        );

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join dispatch smoke UI thread");
    }
}
