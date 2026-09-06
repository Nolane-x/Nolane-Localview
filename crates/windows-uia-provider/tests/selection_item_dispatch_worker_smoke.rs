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
            Foundation::{LPARAM, WPARAM},
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, LB_ADDSTRING,
                MSG, PM_REMOVE, PeekMessageW, SW_SHOW, SendMessageW, ShowWindow, TranslateMessage,
                WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn dispatches_selection_item_on_the_exact_retained_list_item() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-selection-item-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView SelectionItem Dispatch Smoke"),
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
                    .expect("create SelectionItem fixture parent")
                };
                let listbox = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("LISTBOX"),
                        w!(""),
                        WS_CHILD | WS_VISIBLE,
                        32,
                        48,
                        280,
                        120,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create SelectionItem fixture listbox")
                };
                unsafe {
                    SendMessageW(
                        listbox,
                        LB_ADDSTRING,
                        WPARAM(0),
                        LPARAM(w!("Alpha").as_ptr() as isize),
                    );
                    SendMessageW(
                        listbox,
                        LB_ADDSTRING,
                        WPARAM(0),
                        LPARAM(w!("Beta").as_ptr() as isize),
                    );
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish SelectionItem fixture HWND");

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
                    DestroyWindow(window).expect("destroy SelectionItem fixture");
                }
            })
            .expect("spawn responsive SelectionItem fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive SelectionItem fixture HWND");
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
            .expect("attach exact SelectionItem fixture target");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-selection-item-smoke:1".into(),
                    surface_scope: "fixture:win32-listbox".into(),
                },
            )
            .expect("publish retained semantic snapshot before SelectionItem dispatch");
        let item = snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some("Beta"))
            .expect("real Win32 list item Beta must be present in the semantic snapshot");
        assert_eq!(
            WindowsUiaActionCapabilities::from_node(item)
                .support_for(WindowsUiaPattern::SelectionItem),
            WindowsUiaPatternSupport::Supported,
            "fixture item must publish live SelectionItem support before dispatch"
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
            preparation_receipt_ref: "prepare:windows-uia-selection-item-smoke:1".into(),
            snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            element_ref: item.element_ref.clone(),
            required_pattern: WindowsUiaPattern::SelectionItem,
            context_requirements: requirements,
        };

        let receipt = worker
            .dispatch_pattern(&attachment, request)
            .expect("Select the exact retained live UIA SelectionItem on its owning MTA worker");

        assert_eq!(receipt.dispatch_attempt_ref, dispatch_attempt_ref);
        assert_eq!(receipt.action_id, action_id);
        assert_eq!(receipt.required_pattern, WindowsUiaPattern::SelectionItem);
        assert_eq!(receipt.element_ref, item.element_ref);
        assert_eq!(
            receipt.transport_result,
            TransportResult::DeliveredToExecutor
        );
        assert_eq!(receipt.dispatch_result, DispatchResult::DispatchedFull);

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join SelectionItem fixture UI thread");
    }
}
