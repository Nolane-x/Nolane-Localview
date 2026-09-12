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
    use localview_protocol::{DispatchResult, ReconciliationCompleteness, TransportResult};
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
        WindowsUiaPatternDispatchOperation, WindowsUiaPatternDispatchRequest,
        WindowsUiaPatternSupport, WindowsUiaSnapshotRequest, WindowsUiaWorker,
        WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        Win32::{
            Foundation::{LPARAM, WPARAM},
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CBS_DROPDOWNLIST, CB_ADDSTRING, CB_SETCURSEL, CW_USEDEFAULT, CreateWindowExW,
                DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SW_SHOW,
                SendMessageW, ShowWindow, TranslateMessage, WINDOW_STYLE, WS_CHILD,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const EXPAND_COLLAPSE_STATE_ATTRIBUTE: &str = "windows_uia.expand_collapse.state";

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    fn expand_returns_only_after_a_fresh_snapshot_observes_expanded_state() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-expand-collapse-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView ExpandCollapse Dispatch Smoke"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        500,
                        300,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create ExpandCollapse fixture parent")
                };
                let combo_style =
                    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST as u32);
                let combo = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("COMBOBOX"),
                        w!("LocalView ExpandCollapse"),
                        combo_style,
                        32,
                        48,
                        300,
                        180,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create ExpandCollapse fixture ComboBox")
                };
                unsafe {
                    SendMessageW(
                        combo,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(w!("Alpha").as_ptr() as isize)),
                    );
                    SendMessageW(
                        combo,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(w!("Beta").as_ptr() as isize)),
                    );
                    SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(0)), Some(LPARAM(0)));
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish ExpandCollapse fixture HWND");

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
                    DestroyWindow(window).expect("destroy ExpandCollapse fixture");
                }
            })
            .expect("spawn responsive ExpandCollapse fixture UI thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive ExpandCollapse fixture HWND");
        let worker = WindowsUiaWorker::spawn(WindowsUiaWorkerConfig {
            snapshot_budget: SnapshotBudget {
                max_nodes: 32,
                max_depth: 4,
                // Keep the property ceiling aligned with the explicit node ceiling:
                // 32 bounded nodes × 19 tracked properties per semantic node.
                max_properties: 608,
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
            .expect("attach exact ExpandCollapse fixture target");
        let initial = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-expand-collapse-smoke:1".into(),
                    surface_scope: "fixture:win32-combobox".into(),
                },
            )
            .expect("publish retained semantic snapshot before Expand dispatch");
        assert_eq!(
            initial.completeness(),
            ReconciliationCompleteness::Established,
            "collapsed fixture snapshot must begin complete; debt={:?}, usage={:?}",
            initial.incompleteness_debt(),
            initial.resource_usage()
        );
        let combo = initial
            .nodes()
            .iter()
            .find(|node| {
                node.class_name.as_deref() == Some("ComboBox")
                    && WindowsUiaActionCapabilities::from_node(node)
                        .support_for(WindowsUiaPattern::ExpandCollapse)
                        == WindowsUiaPatternSupport::Supported
            })
            .expect("real ComboBox must publish ExpandCollapse support");
        assert_eq!(
            combo.attributes
                .get(EXPAND_COLLAPSE_STATE_ATTRIBUTE)
                .map(String::as_str),
            Some("collapsed"),
            "fixture must begin from a provider-observed collapsed state"
        );

        let dispatch_attempt_ref = Uuid::new_v4();
        let action_id = Uuid::new_v4();
        let request = WindowsUiaPatternDispatchRequest {
            dispatch_attempt_ref,
            action_id,
            preparation_journal_sequence: 1,
            preparation_receipt_ref: "prepare:windows-uia-expand-collapse-smoke:1".into(),
            snapshot_cut_ref: initial.snapshot_cut_ref().into(),
            provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
            target_incarnation_ref: attachment.target_incarnation_ref().clone(),
            element_ref: combo.element_ref.clone(),
            required_pattern: WindowsUiaPattern::ExpandCollapse,
            dispatch_operation: WindowsUiaPatternDispatchOperation::Expand,
            context_requirements: WindowsUiaDispatchContextRequirements {
                require_foreground_target: false,
                require_exact_element_focus: false,
                require_no_modal_blocker: false,
            },
        };
        let receipt = worker
            .dispatch_pattern(&attachment, request)
            .expect("Expand the exact retained live UIA ComboBox on its owning MTA worker");
        assert_eq!(receipt.dispatch_attempt_ref, dispatch_attempt_ref);
        assert_eq!(receipt.action_id, action_id);
        assert_eq!(receipt.required_pattern, WindowsUiaPattern::ExpandCollapse);
        assert_eq!(
            receipt.dispatch_operation,
            WindowsUiaPatternDispatchOperation::Expand
        );
        assert_eq!(
            receipt.transport_result,
            TransportResult::DeliveredToExecutor
        );
        assert_eq!(receipt.dispatch_result, DispatchResult::DispatchedFull);

        let postdispatch = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-expand-collapse-smoke:2".into(),
                    surface_scope: "fixture:win32-combobox".into(),
                },
            )
            .expect("publish fresh semantic snapshot after blocking Expand dispatch");
        let expanded_combo = postdispatch
            .nodes()
            .iter()
            .find(|node| node.class_name.as_deref() == Some("ComboBox"))
            .expect("ComboBox must remain present after Expand dispatch");
        assert_eq!(
            expanded_combo
                .attributes
                .get(EXPAND_COLLAPSE_STATE_ATTRIBUTE)
                .map(String::as_str),
            Some("expanded"),
            "blocking UIA Expand must be followed by fresh provider evidence of expanded state"
        );
        assert_eq!(
            postdispatch.completeness(),
            ReconciliationCompleteness::Established,
            "expanded fresh snapshot must remain verifier-eligible; debt={:?}, usage={:?}",
            postdispatch.incompleteness_debt(),
            postdispatch.resource_usage()
        );
        assert!(
            postdispatch.incompleteness_debt().is_empty()
                && !postdispatch.resource_usage().incomplete,
            "expanded fresh snapshot must carry no hidden incompleteness; debt={:?}, usage={:?}",
            postdispatch.incompleteness_debt(),
            postdispatch.resource_usage()
        );

        // Mirror the second consequential plan: the next fresh provider cut must
        // be able to rebind the exact ComboBox identity from the post-dispatch cut.
        let expanded_identity = expanded_combo
            .element_ref
            .opaque_provider_element_id
            .clone();
        let planning = worker
            .snapshot(
                &attachment,
                WindowsUiaSnapshotRequest {
                    snapshot_cut_ref: "cut:windows-uia-expand-collapse-smoke:3".into(),
                    surface_scope: "fixture:win32-combobox".into(),
                },
            )
            .expect("publish a second fresh semantic snapshot while ComboBox remains expanded");
        assert_eq!(
            planning.completeness(),
            ReconciliationCompleteness::Established,
            "second expanded snapshot must remain complete; debt={:?}, usage={:?}",
            planning.incompleteness_debt(),
            planning.resource_usage()
        );
        let collisions = planning
            .nodes()
            .iter()
            .filter(|node| node.element_ref.opaque_provider_element_id == expanded_identity)
            .map(|node| {
                format!(
                    "depth={} parent={:?} class={:?} role={:?} control_type={:?} automation_id={:?} name={:?} state={:?}",
                    node.depth,
                    node.parent_index,
                    node.class_name,
                    node.role,
                    node.control_type,
                    node.automation_id,
                    node.name,
                    node.attributes.get(EXPAND_COLLAPSE_STATE_ATTRIBUTE)
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            collisions.len(),
            1,
            "the exact expanded ComboBox RuntimeId must identify one semantic node in the next fresh cut; opaque_id={expanded_identity}; collisions={collisions:?}"
        );

        stop.store(true, Ordering::Release);
        ui_thread
            .join()
            .expect("join ExpandCollapse fixture UI thread");
    }
}
