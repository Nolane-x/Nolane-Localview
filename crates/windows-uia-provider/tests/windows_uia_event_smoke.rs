#[cfg(windows)]
mod windows_event_smoke {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use localview_native_provider::{ProviderEventOrdering, SnapshotBudget, UserSelectedWindowTarget};
    use localview_windows_uia_provider::{
        WindowsUiaEventKind, WindowsUiaEventSubscriptionOptions, WindowsUiaWorker,
        WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        core::w,
        Win32::{
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, SetWindowTextW,
                ShowWindow, TranslateMessage, CW_USEDEFAULT, HWND, MSG, PM_REMOVE, SW_SHOW,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn streams_real_property_changes_through_a_bounded_subscription() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-event-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView UIA Event Before"),
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
                    .expect("create Win32 UIA event fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish event fixture HWND");

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
                    DestroyWindow(window).expect("destroy Win32 UIA event fixture");
                }
            })
            .expect("spawn responsive Win32 event fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive event fixture HWND");
        let worker = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 32,
                max_depth: 4,
                max_properties: 256,
            },
            command_timeout: Duration::from_secs(5),
        })
        .expect("spawn Windows UIA MTA worker");
        let attachment = worker
            .attach(UserSelectedWindowTarget {
                native_window_handle: window_handle,
                expected_process_id: unsafe { GetCurrentProcessId() },
                selection_nonce: Uuid::new_v4(),
            })
            .expect("attach event fixture");

        let subscription = worker
            .subscribe_events(
                &attachment,
                WindowsUiaEventSubscriptionOptions { capacity: 16 },
            )
            .expect("register bounded UIA event subscription");
        assert_eq!(subscription.sequence_baseline(), 0);
        assert_eq!(
            subscription.reliability_profile().ordering,
            ProviderEventOrdering::OpaqueBestEffort
        );
        assert!(!subscription.reliability_profile().global_polling_required);

        unsafe {
            SetWindowTextW(
                HWND(window_handle as isize as *mut _),
                w!("LocalView UIA Event After"),
            )
            .expect("mutate live fixture name");
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut observed = None;
        while Instant::now() < deadline {
            let drained = worker
                .drain_events(&subscription, 32)
                .expect("drain UIA event subscription");
            if let Some(event) = drained.events.into_iter().find(|event| {
                matches!(event.kind, WindowsUiaEventKind::PropertyChanged { .. })
            }) {
                observed = Some(event);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let event = observed.expect("real UIA name mutation must surface as a property event");
        assert!(event.sequence > 0);
        assert_eq!(
            &event.provider_incarnation_ref,
            attachment.provider_incarnation_ref()
        );
        assert_eq!(
            &event.target_incarnation_ref,
            attachment.target_incarnation_ref()
        );

        worker
            .unsubscribe_events(subscription)
            .expect("remove UIA handlers symmetrically");

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join event fixture UI thread");
    }
}
