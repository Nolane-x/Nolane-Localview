#[cfg(windows)]
mod windows_smoke {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread,
        time::Duration,
    };

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_windows_uia_provider::{
        WindowsUiaDispatchContextRequest, WindowsUiaDispatchContextRequirements,
        WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        core::w,
        Win32::{
            System::Threading::GetCurrentProcessId,
            UI::{
                Input::KeyboardAndMouse::SetFocus,
                WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW,
                    SetForegroundWindow, ShowWindow, TranslateMessage, CW_USEDEFAULT, MSG,
                    PM_REMOVE, SW_SHOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                },
            },
        },
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn observes_dispatch_context_on_the_same_mta_as_the_exact_retained_element() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-context-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView UIA Context Smoke"),
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
                    .expect("create deterministic Win32 UIA context fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                    let _ = SetForegroundWindow(window);
                    let _ = SetFocus(Some(window));
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish context smoke HWND");

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
                    DestroyWindow(window).expect("destroy Win32 UIA context fixture");
                }
            })
            .expect("spawn responsive Win32 context smoke UI thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive live context smoke HWND");

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
            .expect("attach exact user-selected context target");
        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-context-smoke:1".into(),
                    surface_scope: "fixture:win32-context-button".into(),
                },
            )
            .expect("observe real context UIA semantic snapshot");
        let fixture_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.name
                    .as_deref()
                    .is_some_and(|name| name.contains("LocalView UIA Context Smoke"))
            })
            .expect("context fixture must be retained in the exact snapshot");

        let requirements = WindowsUiaDispatchContextRequirements {
            require_foreground_target: true,
            require_exact_element_focus: true,
            require_no_modal_blocker: true,
        };
        let receipt = worker
            .revalidate_dispatch_context(
                &attachment,
                WindowsUiaDispatchContextRequest {
                    snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
                    element_ref: fixture_node.element_ref.clone(),
                    requirements,
                },
            )
            .expect("exact live element + current Windows context must pass");

        assert_eq!(receipt.snapshot_cut_ref, snapshot.snapshot_cut_ref());
        assert_eq!(receipt.element_ref, fixture_node.element_ref);
        assert_eq!(receipt.requirements, requirements);
        assert_eq!(
            &receipt.provider_incarnation_ref,
            attachment.provider_incarnation_ref()
        );
        assert_eq!(
            &receipt.target_incarnation_ref,
            attachment.target_incarnation_ref()
        );
        assert_eq!(receipt.observation.target_window_handle, window_handle);
        assert_eq!(receipt.observation.target_process_id, process_id);
        assert_eq!(receipt.observation.foreground_window_handle, Some(window_handle));
        assert_eq!(receipt.observation.foreground_process_id, Some(process_id));
        assert_eq!(receipt.observation.exact_element_focused, Some(true));
        assert_eq!(receipt.observation.modal_blocker_window_handle, None);
        assert!(
            !WindowsUiaWorker::capabilities().write_actions
                && !WindowsUiaWorker::capabilities().input_dispatch,
            "context observation must remain read-only"
        );

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join context smoke UI thread");
    }
}
