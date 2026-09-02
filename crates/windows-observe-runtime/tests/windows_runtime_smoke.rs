#[cfg(windows)]
mod windows_runtime_smoke {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use localview_live_bridge::LiveBridge;
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{EventContinuityState, ReconciliationCompleteness};
    use localview_windows_observe_runtime::{
        spawn_windows_uia_runtime_manager, WindowsObserveRuntimeConfig,
    };
    use localview_windows_uia_provider::WindowsUiaWorkerConfig;
    use uuid::Uuid;
    use windows::{
        core::w,
        Win32::{
            Foundation::HWND,
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, SetWindowTextW,
                ShowWindow, TranslateMessage, CW_USEDEFAULT, MSG, PM_REMOVE, SW_SHOW,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_win32_target_flows_through_runtime_binding_reconciliation_and_detach() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-runtime-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView Runtime Before"),
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
                    .expect("create Win32 runtime fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish runtime fixture HWND");

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
                    DestroyWindow(window).expect("destroy Win32 runtime fixture");
                }
            })
            .expect("spawn responsive Win32 runtime fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive runtime fixture HWND");
        let bridge = LiveBridge::new(128, 16);
        let manager = spawn_windows_uia_runtime_manager(
            bridge.clone(),
            WindowsUiaWorkerConfig {
                snapshot_budget: SnapshotBudget {
                    max_nodes: 32,
                    max_depth: 4,
                    max_properties: 256,
                },
                command_timeout: Duration::from_secs(5),
            },
            WindowsObserveRuntimeConfig {
                event_capacity: 16,
                drain_limit: 32,
            },
        )
        .expect("spawn concrete Windows UIA observe runtime");

        let session_id = Uuid::new_v4();
        let status = manager
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: unsafe { GetCurrentProcessId() },
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach real Win32 target through runtime manager");
        assert_eq!(status.event_continuity, EventContinuityState::OrderingOpaque);
        assert_eq!(status.generation, 1);
        assert_eq!(
            status.current_snapshot_completeness,
            Some(ReconciliationCompleteness::Established)
        );

        unsafe {
            SetWindowTextW(
                HWND(window_handle as isize as *mut _),
                w!("LocalView Runtime After"),
            )
            .expect("mutate runtime fixture name");
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut observed_property_change = false;
        while Instant::now() < deadline {
            manager
                .drain_once(session_id)
                .await
                .expect("drain concrete Windows UIA runtime");
            let recent = bridge.recent(session_id, 32).await;
            if recent.iter().any(|event| {
                event.payload.get("native_provider").and_then(|value| value.as_str())
                    == Some("windows_uia")
                    && event.payload.get("native_event").and_then(|value| value.as_str())
                        == Some("property_changed")
            }) {
                observed_property_change = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            observed_property_change,
            "real UIA name mutation must flow through runtime manager into LiveBridge"
        );

        manager
            .release(session_id)
            .await
            .expect("detach runtime observation and unregister UIA handler");
        assert!(manager.status(session_id).await.is_none());
        assert!(bridge.observation_status(session_id).await.is_none());

        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join runtime fixture UI thread");
    }
}
