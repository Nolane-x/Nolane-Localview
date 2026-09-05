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
    use localview_protocol::ReconciliationCompleteness;
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaElementLeaseRequest, WindowsUiaPattern,
        WindowsUiaPatternSupport, WindowsUiaSnapshotRequest, WindowsUiaWorker,
        WindowsUiaWorkerConfig, WindowsUiaWorkerError,
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
    fn observes_a_real_user_selected_win32_window_through_the_mta_worker() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView UIA Smoke Save"),
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
                    .expect("create deterministic Win32 UIA fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish smoke HWND");

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
                    DestroyWindow(window).expect("destroy Win32 UIA fixture");
                }
            })
            .expect("spawn responsive Win32 smoke UI thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive live smoke HWND");

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
        let selection = UserSelectedWindowTarget {
            native_window_handle: window_handle,
            expected_process_id: process_id,
            selection_nonce: Uuid::new_v4(),
        };
        let attachment = worker
            .attach(selection)
            .expect("attach exact user-selected Win32 target");
        assert_eq!(attachment.fingerprint().process_id, process_id);
        assert_eq!(attachment.fingerprint().native_window_handle, window_handle);

        let snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-smoke:1".into(),
                    surface_scope: "fixture:win32-button".into(),
                },
            )
            .expect("observe real UIA semantic snapshot");

        assert_eq!(
            snapshot.completeness(),
            ReconciliationCompleteness::Established
        );
        assert!(!snapshot.nodes().is_empty());
        let fixture_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.name
                    .as_deref()
                    .is_some_and(|name| name.contains("LocalView UIA Smoke"))
            })
            .expect("real Win32 button must be present in the semantic snapshot");
        let action_capabilities = WindowsUiaActionCapabilities::from_node(fixture_node);
        assert_eq!(
            action_capabilities.support_for(WindowsUiaPattern::Invoke),
            WindowsUiaPatternSupport::Supported,
            "real Win32 BUTTON must publish explicit Invoke-pattern support evidence"
        );
        assert_eq!(
            snapshot.provider_incarnation_ref(),
            attachment.provider_incarnation_ref()
        );
        assert_eq!(
            snapshot.target_incarnation_ref(),
            attachment.target_incarnation_ref()
        );

        let lease = worker
            .bind_element_lease(
                &attachment,
                WindowsUiaElementLeaseRequest {
                    snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
                    element_ref: fixture_node.element_ref.clone(),
                },
            )
            .expect("bind exact current COM element retained by the MTA worker");
        assert_eq!(lease.snapshot_cut_ref, snapshot.snapshot_cut_ref());
        assert_eq!(lease.element_ref, fixture_node.element_ref);
        assert_eq!(
            &lease.provider_incarnation_ref,
            attachment.provider_incarnation_ref()
        );
        assert_eq!(
            &lease.target_incarnation_ref,
            attachment.target_incarnation_ref()
        );

        let stale_request = WindowsUiaElementLeaseRequest {
            snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            element_ref: fixture_node.element_ref.clone(),
        };
        let second_snapshot = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-smoke:2".into(),
                    surface_scope: "fixture:win32-button".into(),
                },
            )
            .expect("publish a newer semantic snapshot and replace retained leases");
        assert_eq!(
            worker
                .bind_element_lease(&attachment, stale_request)
                .unwrap_err(),
            WindowsUiaWorkerError::ElementLeaseSnapshotExpired {
                requested_cut: snapshot.snapshot_cut_ref().into(),
                current_cut: second_snapshot.snapshot_cut_ref().into(),
            }
        );

        let second_fixture_node = second_snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.name
                    .as_deref()
                    .is_some_and(|name| name.contains("LocalView UIA Smoke"))
            })
            .expect("real Win32 button must remain present in the second snapshot");
        let second_lease = worker
            .bind_element_lease(
                &attachment,
                WindowsUiaElementLeaseRequest {
                    snapshot_cut_ref: second_snapshot.snapshot_cut_ref().into(),
                    element_ref: second_fixture_node.element_ref.clone(),
                },
            )
            .expect("bind exact element from the current snapshot");
        assert_eq!(second_lease.element_ref, second_fixture_node.element_ref);

        let mut forged_element = second_fixture_node.element_ref.clone();
        forged_element
            .opaque_provider_element_id
            .push_str(":forged");
        assert_eq!(
            worker
                .bind_element_lease(
                    &attachment,
                    WindowsUiaElementLeaseRequest {
                        snapshot_cut_ref: second_snapshot.snapshot_cut_ref().into(),
                        element_ref: forged_element,
                    },
                )
                .unwrap_err(),
            WindowsUiaWorkerError::ElementLeaseNotFound
        );

        assert!(
            !WindowsUiaWorker::capabilities().write_actions
                && !WindowsUiaWorker::capabilities().input_dispatch,
            "element leasing must not silently enable UIA writes or input dispatch"
        );

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join smoke UI thread");
    }
}
