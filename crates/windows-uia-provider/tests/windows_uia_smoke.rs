#[cfg(windows)]
mod windows_smoke {
    use std::time::Duration;

    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::ReconciliationCompleteness;
    use localview_windows_uia_provider::{
        WindowsUiaSnapshotRequest, WindowsUiaWorker, WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        core::w,
        Win32::{
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, ShowWindow, CW_USEDEFAULT, SW_SHOW,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn observes_a_real_user_selected_win32_window_through_the_mta_worker() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

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
            native_window_handle: window.0 as usize as u64,
            expected_process_id: process_id,
            selection_nonce: Uuid::new_v4(),
        };
        let attachment = worker
            .attach(selection)
            .expect("attach exact user-selected Win32 target");
        assert_eq!(attachment.fingerprint().process_id, process_id);
        assert_eq!(
            attachment.fingerprint().native_window_handle,
            window.0 as usize as u64
        );

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
        assert!(snapshot.nodes().iter().any(|node| {
            node.name
                .as_deref()
                .is_some_and(|name| name.contains("LocalView UIA Smoke"))
        }));
        assert_eq!(
            snapshot.provider_incarnation_ref(),
            attachment.provider_incarnation_ref()
        );
        assert_eq!(
            snapshot.target_incarnation_ref(),
            attachment.target_incarnation_ref()
        );

        unsafe {
            DestroyWindow(window).expect("destroy Win32 UIA fixture");
        }
    }
}
