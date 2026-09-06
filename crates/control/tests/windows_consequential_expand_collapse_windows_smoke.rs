#[cfg(windows)]
mod windows_consequential_expand_collapse_windows_smoke {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header::AUTHORIZATION},
    };
    use chrono::Utc;
    use localview_control::{
        ControlState, configure_windows_consequential_control_for_sessions,
        configure_windows_observe_runtime_for_sessions, router,
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
            Foundation::{LPARAM, WPARAM},
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CBS_DROPDOWNLIST, CB_ADDSTRING, CB_SETCURSEL, CW_USEDEFAULT, CreateWindowExW,
                DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, SW_SHOW,
                SendMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, WINDOW_STYLE,
                WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const EXPAND_COLLAPSE_STATE_ATTRIBUTE: &str = "windows_uia.expand_collapse.state";

    fn discovered() -> DiscoveredServer {
        DiscoveredServer {
            candidate: ListenerCandidate {
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port: 5913,
                    scheme: "http".into(),
                },
                pid: Some(95),
                process_name: Some("localview-expand-collapse-control-smoke".into()),
                command: Some("localview-expand-collapse-control-smoke".into()),
                cwd: Some("C:/localview-expand-collapse-control-smoke".into()),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("LocalViewExpandCollapseControlSmoke".into()),
                title: None,
                hmr_detected: false,
                evidence: Default::default(),
            },
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read ExpandCollapse control response body");
        serde_json::from_slice(&bytes).expect("ExpandCollapse control response must be JSON")
    }

    fn state_postcondition(state: &str) -> String {
        NativeSemanticPostconditionContractV1 {
            expectation: NativeSemanticPostconditionExpectation::Present,
            matcher: NativeSemanticNodeMatcherV1 {
                class_name: Some("ComboBox".into()),
                attributes: BTreeMap::from([(
                    EXPAND_COLLAPSE_STATE_ATTRIBUTE.into(),
                    state.into(),
                )]),
                ..Default::default()
            },
        }
        .to_contract_ref()
        .expect("encode typed ExpandCollapse-state postcondition contract")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_http_expand_then_collapse_requires_distinct_server_owned_operations_and_fresh_state() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-expand-collapse-control-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView ExpandCollapse Control Smoke"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        520,
                        320,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 ExpandCollapse control-path parent fixture")
                };
                let combo_style =
                    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST as u32);
                let combo = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("COMBOBOX"),
                        w!("LocalView ExpandCollapse"),
                        combo_style,
                        36,
                        48,
                        300,
                        180,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 ExpandCollapse control-path ComboBox")
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
                    let _ = SetForegroundWindow(window);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish ExpandCollapse control-path fixture HWND");

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
                    DestroyWindow(window).expect("destroy ExpandCollapse control-path fixture");
                }
            })
            .expect("spawn responsive ExpandCollapse control-path fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive ExpandCollapse control-path fixture HWND");

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
                        // Keep the property ceiling aligned with the explicit node ceiling:
                        // 32 bounded nodes × 17 tracked properties per semantic node.
                        max_properties: 544,
                    },
                    command_timeout: Duration::from_secs(5),
                },
                WindowsObserveRuntimeConfig {
                    event_capacity: 16,
                    drain_limit: 32,
                },
            )
            .expect("spawn concrete Windows UIA ExpandCollapse control-path runtime"),
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
            .expect("attach real Win32 ExpandCollapse target through control-path runtime");

        let initial_snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("attached ExpandCollapse runtime must expose current semantic snapshot");
        let initial_snapshot_cut = initial_snapshot.snapshot_cut_ref().to_owned();
        let combo = initial_snapshot
            .nodes()
            .iter()
            .find(|node| {
                node.class_name.as_deref() == Some("ComboBox")
                    && WindowsUiaActionCapabilities::from_node(node)
                        .support_for(WindowsUiaPattern::ExpandCollapse)
                        == WindowsUiaPatternSupport::Supported
            })
            .cloned()
            .expect("real ComboBox must publish ExpandCollapse support");
        assert_eq!(
            combo.element_ref.acquisition_cut_ref, initial_snapshot_cut,
            "the first HTTP Expand request deliberately starts from the initial cached element ref"
        );

        let journal_path = std::env::temp_dir().join(format!(
            "localview-windows-expand-collapse-control-consequential-{}.jsonl",
            Uuid::new_v4()
        ));
        let journal = Arc::new(
            ConsequentialJournal::open(&journal_path)
                .await
                .expect("open ExpandCollapse control-path consequential journal"),
        );
        configure_windows_observe_runtime_for_sessions(&sessions, Some(runtime.clone()));
        configure_windows_consequential_control_for_sessions(&sessions, Some(journal.clone()));

        let app = router(ControlState {
            token: Arc::from("expand-collapse-control-smoke-token"),
            sessions: sessions.clone(),
            observations: ObservationBus::new(32),
            live: live.clone(),
            evidence: EvidenceStore::default(),
            paused: Arc::new(AtomicBool::new(false)),
        });

        let expand_plan_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/expand/plan"
                    ))
                    .header(AUTHORIZATION, "Bearer expand-collapse-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "element_ref": combo.element_ref,
                            "expected_postcondition_contract_refs": [state_postcondition("expanded")],
                        })
                        .to_string(),
                    ))
                    .expect("build Expand control-path plan request"),
            )
            .await
            .expect("run Expand control-path plan request");
        assert_eq!(expand_plan_response.status(), StatusCode::CREATED);
        let expand_plan = response_json(expand_plan_response).await;
        assert_eq!(expand_plan["operation"], "expand");
        assert_eq!(expand_plan["risk_class"], "s4_destructive_or_irreversible");
        assert_eq!(expand_plan["idempotency_class"], "irreversible");
        let expand_action_id = Uuid::parse_str(
            expand_plan["action_id"]
                .as_str()
                .expect("Expand plan response must contain action_id"),
        )
        .expect("parse planned Expand action_id");
        let expand_confirmation_ref = expand_plan["confirmation_ref"]
            .as_str()
            .expect("Expand plan response must contain confirmation_ref")
            .to_owned();
        let expand_precondition_cut = expand_plan["precondition_snapshot_cut_ref"]
            .as_str()
            .expect("Expand plan response must contain precondition cut")
            .to_owned();
        assert_ne!(
            expand_precondition_cut, initial_snapshot_cut,
            "Expand planning must re-observe the provider instead of admitting the cached initial cut"
        );
        assert!(
            live.take_public_actions(session_id, 16).await.is_empty(),
            "Expand consequential actions must never enter the legacy V1-V3 executor queue"
        );

        let expand_confirm_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/{expand_action_id}/confirm"
                    ))
                    .header(AUTHORIZATION, "Bearer expand-collapse-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"confirmation_ref": expand_confirmation_ref}).to_string(),
                    ))
                    .expect("build Expand control-path confirmation request"),
            )
            .await
            .expect("run Expand control-path confirmation request");
        assert_eq!(
            expand_confirm_response.status(),
            StatusCode::OK,
            "Expand must not commit until fresh provider evidence proves expanded state"
        );
        let expanded = response_json(expand_confirm_response).await;
        assert_eq!(expanded["status"], "committed");
        assert_eq!(expanded["world_outcome"], "verified_expected");
        assert_eq!(expanded["confirmation_consumed"], true);
        assert_eq!(expanded["retry_allowed"], false);
        assert_eq!(
            journal.recovery_state(expand_action_id).await,
            Some(ConsequentialRecoveryState::Committed)
        );
        let expand_receipt = journal
            .latest_action_postcondition_receipt(expand_action_id)
            .await
            .expect("Expand commit must retain a postcondition receipt");
        assert_eq!(
            expand_receipt.verdict,
            ActionPostconditionVerdict::VerifiedExpected
        );
        assert_ne!(
            expand_receipt.observation_snapshot_cut_ref, expand_precondition_cut,
            "Expand verification must use a fresh post-dispatch observation cut"
        );

        let expanded_snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("verified Expand postcondition snapshot must remain current");
        let expanded_combo = expanded_snapshot
            .nodes()
            .iter()
            .find(|node| node.class_name.as_deref() == Some("ComboBox"))
            .cloned()
            .expect("ComboBox must remain present after Expand dispatch");
        assert_eq!(
            expanded_combo
                .attributes
                .get(EXPAND_COLLAPSE_STATE_ATTRIBUTE)
                .map(String::as_str),
            Some("expanded"),
            "fresh provider evidence must prove the exact ComboBox is expanded"
        );

        let collapse_plan_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/collapse/plan"
                    ))
                    .header(AUTHORIZATION, "Bearer expand-collapse-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "element_ref": expanded_combo.element_ref,
                            "expected_postcondition_contract_refs": [state_postcondition("collapsed")],
                        })
                        .to_string(),
                    ))
                    .expect("build Collapse control-path plan request"),
            )
            .await
            .expect("run Collapse control-path plan request");
        assert_eq!(collapse_plan_response.status(), StatusCode::CREATED);
        let collapse_plan = response_json(collapse_plan_response).await;
        assert_eq!(collapse_plan["operation"], "collapse");
        assert_eq!(collapse_plan["risk_class"], "s4_destructive_or_irreversible");
        assert_eq!(collapse_plan["idempotency_class"], "irreversible");
        let collapse_action_id = Uuid::parse_str(
            collapse_plan["action_id"]
                .as_str()
                .expect("Collapse plan response must contain action_id"),
        )
        .expect("parse planned Collapse action_id");
        let collapse_confirmation_ref = collapse_plan["confirmation_ref"]
            .as_str()
            .expect("Collapse plan response must contain confirmation_ref")
            .to_owned();
        let collapse_precondition_cut = collapse_plan["precondition_snapshot_cut_ref"]
            .as_str()
            .expect("Collapse plan response must contain precondition cut")
            .to_owned();
        assert_ne!(
            collapse_precondition_cut,
            expanded_snapshot.snapshot_cut_ref(),
            "Collapse planning must take a distinct fresh planning cut after Expand verification"
        );
        assert!(
            live.take_public_actions(session_id, 16).await.is_empty(),
            "Collapse consequential actions must never enter the legacy V1-V3 executor queue"
        );

        let collapse_confirm_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/{collapse_action_id}/confirm"
                    ))
                    .header(AUTHORIZATION, "Bearer expand-collapse-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"confirmation_ref": collapse_confirmation_ref})
                            .to_string(),
                    ))
                    .expect("build Collapse control-path confirmation request"),
            )
            .await
            .expect("run Collapse control-path confirmation request");
        assert_eq!(
            collapse_confirm_response.status(),
            StatusCode::OK,
            "Collapse must not commit until fresh provider evidence proves collapsed state"
        );
        let collapsed = response_json(collapse_confirm_response).await;
        assert_eq!(collapsed["status"], "committed");
        assert_eq!(collapsed["world_outcome"], "verified_expected");
        assert_eq!(collapsed["confirmation_consumed"], true);
        assert_eq!(collapsed["retry_allowed"], false);
        assert_eq!(
            journal.recovery_state(collapse_action_id).await,
            Some(ConsequentialRecoveryState::Committed)
        );
        let collapse_receipt = journal
            .latest_action_postcondition_receipt(collapse_action_id)
            .await
            .expect("Collapse commit must retain a postcondition receipt");
        assert_eq!(
            collapse_receipt.verdict,
            ActionPostconditionVerdict::VerifiedExpected
        );
        assert_ne!(
            collapse_receipt.observation_snapshot_cut_ref, collapse_precondition_cut,
            "Collapse verification must use a fresh post-dispatch observation cut"
        );
        let collapsed_snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("verified Collapse postcondition snapshot must remain current");
        let collapsed_combo = collapsed_snapshot
            .nodes()
            .iter()
            .find(|node| node.class_name.as_deref() == Some("ComboBox"))
            .expect("ComboBox must remain present after Collapse dispatch");
        assert_eq!(
            collapsed_combo
                .attributes
                .get(EXPAND_COLLAPSE_STATE_ATTRIBUTE)
                .map(String::as_str),
            Some("collapsed"),
            "fresh provider evidence must prove the exact ComboBox is collapsed"
        );

        runtime
            .release(session_id)
            .await
            .expect("release ExpandCollapse control-path runtime after verified commits");
        configure_windows_observe_runtime_for_sessions(&sessions, None);
        configure_windows_consequential_control_for_sessions(&sessions, None);
        stop.store(true, Ordering::Release);
        ui_thread
            .join()
            .expect("join ExpandCollapse control-path fixture UI thread");

        for action_id in [expand_action_id, collapse_action_id] {
            let _ = std::fs::remove_file(format!(
                "{}.operation-{action_id}.json",
                journal_path.display()
            ));
        }
        let _ = std::fs::remove_file(journal_path);
    }
}
