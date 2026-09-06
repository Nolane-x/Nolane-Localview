#[cfg(windows)]
mod windows_consequential_control_windows_smoke {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        },
        thread,
        time::Duration,
    };

    use axum::{
        body::{to_bytes, Body},
        http::{header::AUTHORIZATION, Request, StatusCode},
    };
    use chrono::Utc;
    use localview_control::{
        configure_windows_consequential_control_for_sessions,
        configure_windows_observe_runtime_for_sessions, router, ControlState,
    };
    use localview_evidence::EvidenceStore;
    use localview_live_bridge::{
        ActionPostconditionVerdict, ConsequentialJournal, ConsequentialRecoveryState, LiveBridge,
    };
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_observation::ObservationBus;
    use localview_protocol::{
        Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
    };
    use localview_sessions::SessionManager;
    use localview_windows_observe_runtime::{
        NativeSemanticNodeMatcherV1, NativeSemanticPostconditionContractV1,
        NativeSemanticPostconditionExpectation, WindowsObserveRuntimeConfig,
        spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaPattern, WindowsUiaPatternSupport,
        WindowsUiaWorkerConfig,
    };
    use tower::ServiceExt;
    use uuid::Uuid;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, WPARAM},
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWLP_WNDPROC, MSG, PM_REMOVE, PeekMessageW, SW_SHOW, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WM_COMMAND,
                WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const BEFORE_TITLE: &str = "LocalView Control Before";
    const AFTER_TITLE: &str = "LocalView Control Invoked";

    unsafe extern "system" fn smoke_parent_wndproc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_COMMAND {
            unsafe {
                let _ = SetWindowTextW(window, w!("LocalView Control Invoked"));
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn discovered() -> DiscoveredServer {
        DiscoveredServer {
            candidate: ListenerCandidate {
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port: 5910,
                    scheme: "http".into(),
                },
                pid: Some(91),
                process_name: Some("localview-control-smoke".into()),
                command: Some("localview-control-smoke".into()),
                cwd: Some("C:/localview-control-smoke".into()),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("LocalViewControlSmoke".into()),
                title: None,
                hmr_detected: false,
                evidence: Default::default(),
            },
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read control response body");
        serde_json::from_slice(&bytes).expect("control response must be JSON")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_http_plan_confirm_invoke_reaches_verified_durable_commit_without_legacy_queue() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-control-consequential-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView Control Before"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        480,
                        240,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 control-path parent fixture")
                };
                unsafe {
                    let _ = SetWindowLongPtrW(
                        window,
                        GWLP_WNDPROC,
                        smoke_parent_wndproc as *const () as isize,
                    );
                }
                let _button = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("Invoke Through Control"),
                        WS_CHILD | WS_VISIBLE,
                        36,
                        56,
                        240,
                        52,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 control-path Invoke button")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                    let _ = SetForegroundWindow(window);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish control-path fixture HWND");

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
                    DestroyWindow(window).expect("destroy Win32 control-path fixture");
                }
            })
            .expect("spawn responsive Win32 control-path fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive control-path fixture HWND");

        let sessions = Arc::new(SessionManager::new(Duration::from_secs(2)));
        let reconcile = sessions.reconcile(vec![discovered()], Utc::now()).await;
        let session_id = reconcile.created[0];
        let live = LiveBridge::new(128, 16);
        let runtime = Arc::new(
            spawn_windows_uia_runtime_manager(
                live.clone(),
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
            .expect("spawn concrete Windows UIA control-path runtime"),
        );
        runtime
            .attach(
                session_id,
                UserSelectedWindowTarget {
                    native_window_handle: window_handle,
                    expected_process_id: unsafe { GetCurrentProcessId() },
                    selection_nonce: Uuid::new_v4(),
                },
            )
            .await
            .expect("attach real Win32 target through control-path runtime");

        let snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("attached runtime must expose current semantic snapshot");
        assert!(
            snapshot
                .nodes()
                .iter()
                .any(|node| node.name.as_deref() == Some(BEFORE_TITLE)),
            "initial control-path snapshot must observe the pre-dispatch world state"
        );
        let invoke_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                WindowsUiaActionCapabilities::from_node(node).support_for(WindowsUiaPattern::Invoke)
                    == WindowsUiaPatternSupport::Supported
            })
            .cloned()
            .expect("real child BUTTON must advertise UIA Invoke support");
        let postcondition_ref = NativeSemanticPostconditionContractV1 {
            expectation: NativeSemanticPostconditionExpectation::Present,
            matcher: NativeSemanticNodeMatcherV1 {
                name: Some(AFTER_TITLE.into()),
                ..Default::default()
            },
        }
        .to_contract_ref()
        .expect("encode typed observable control-path postcondition");

        let journal_path = std::env::temp_dir().join(format!(
            "localview-windows-control-consequential-{}.jsonl",
            Uuid::new_v4()
        ));
        let journal = Arc::new(
            ConsequentialJournal::open(&journal_path)
                .await
                .expect("open control-path consequential journal"),
        );
        configure_windows_observe_runtime_for_sessions(&sessions, Some(runtime.clone()));
        configure_windows_consequential_control_for_sessions(&sessions, Some(journal.clone()));

        let app = router(ControlState {
            token: Arc::from("control-smoke-token"),
            sessions: sessions.clone(),
            observations: ObservationBus::new(32),
            live: live.clone(),
            evidence: EvidenceStore::default(),
            paused: Arc::new(AtomicBool::new(false)),
        });

        let plan_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/invoke/plan"
                    ))
                    .header(AUTHORIZATION, "Bearer control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "element_ref": invoke_node.element_ref,
                            "expected_postcondition_contract_refs": [postcondition_ref],
                        })
                        .to_string(),
                    ))
                    .expect("build control-path plan request"),
            )
            .await
            .expect("run control-path plan request");
        assert_eq!(plan_response.status(), StatusCode::CREATED);
        let plan = response_json(plan_response).await;
        let action_id = Uuid::parse_str(
            plan["action_id"]
                .as_str()
                .expect("plan response must contain action_id"),
        )
        .expect("parse planned action_id");
        let confirmation_ref = plan["confirmation_ref"]
            .as_str()
            .expect("plan response must contain confirmation_ref");
        let precondition_cut = plan["precondition_snapshot_cut_ref"]
            .as_str()
            .expect("plan response must contain precondition cut")
            .to_owned();
        assert!(
            live.take_public_actions(session_id, 16).await.is_empty(),
            "production consequential control actions must never enter the legacy V1-V3 executor queue"
        );

        let confirm_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/{action_id}/confirm"
                    ))
                    .header(AUTHORIZATION, "Bearer control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"confirmation_ref": confirmation_ref}).to_string(),
                    ))
                    .expect("build control-path confirmation request"),
            )
            .await
            .expect("run control-path confirmation request");
        assert_eq!(confirm_response.status(), StatusCode::OK);
        let confirmed = response_json(confirm_response).await;
        assert_eq!(confirmed["status"], "committed");
        assert_eq!(confirmed["world_outcome"], "verified_expected");
        assert_eq!(confirmed["confirmation_consumed"], true);
        assert_eq!(confirmed["retry_allowed"], false);
        assert_eq!(
            journal.recovery_state(action_id).await,
            Some(ConsequentialRecoveryState::Committed)
        );
        let receipt = journal
            .latest_action_postcondition_receipt(action_id)
            .await
            .expect("control-path commit must retain a postcondition receipt");
        assert_eq!(receipt.verdict, ActionPostconditionVerdict::VerifiedExpected);
        assert_ne!(
            receipt.observation_snapshot_cut_ref, precondition_cut,
            "control-path verification must use a fresh post-dispatch observation cut"
        );

        runtime
            .release(session_id)
            .await
            .expect("release control-path runtime after verified commit");
        configure_windows_observe_runtime_for_sessions(&sessions, None);
        configure_windows_consequential_control_for_sessions(&sessions, None);
        stop.store(true, Ordering::Release);
        ui_thread
            .join()
            .expect("join control-path fixture UI thread");

        let _ = std::fs::remove_file(format!(
            "{}.operation-{action_id}.json",
            journal_path.display()
        ));
        let _ = std::fs::remove_file(journal_path);
    }
}
