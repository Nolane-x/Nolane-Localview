#[cfg(windows)]
mod windows_consequential_toggle_windows_smoke {
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
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                BS_AUTOCHECKBOX, CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW,
                MSG, PM_REMOVE, PeekMessageW, SW_SHOW, SetForegroundWindow, ShowWindow,
                TranslateMessage, WINDOW_STYLE, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const TOGGLE_STATE_ATTRIBUTE: &str = "windows_uia.toggle.state";

    fn discovered() -> DiscoveredServer {
        DiscoveredServer {
            candidate: ListenerCandidate {
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port: 5912,
                    scheme: "http".into(),
                },
                pid: Some(94),
                process_name: Some("localview-toggle-control-smoke".into()),
                command: Some("localview-toggle-control-smoke".into()),
                cwd: Some("C:/localview-toggle-control-smoke".into()),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("LocalViewToggleControlSmoke".into()),
                title: None,
                hmr_detected: false,
                evidence: Default::default(),
            },
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read Toggle control response body");
        serde_json::from_slice(&bytes).expect("Toggle control response must be JSON")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_http_plan_confirm_toggle_uses_fresh_toggle_state_before_durable_commit() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-toggle-control-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView Toggle Control Smoke"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        480,
                        280,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 Toggle control-path parent fixture")
                };
                let checkbox_style =
                    WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | BS_AUTOCHECKBOX as u32);
                unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("BUTTON"),
                        w!("LocalView Toggle"),
                        checkbox_style,
                        36,
                        48,
                        280,
                        44,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 Toggle control-path checkbox");
                    let _ = ShowWindow(window, SW_SHOW);
                    let _ = SetForegroundWindow(window);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish Toggle control-path fixture HWND");

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
                    DestroyWindow(window).expect("destroy Toggle control-path fixture");
                }
            })
            .expect("spawn responsive Toggle control-path fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive Toggle control-path fixture HWND");

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
            .expect("spawn concrete Windows UIA Toggle control-path runtime"),
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
            .expect("attach real Win32 Toggle target through control-path runtime");

        let snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("attached Toggle runtime must expose current semantic snapshot");
        let initial_snapshot_cut = snapshot.snapshot_cut_ref().to_owned();
        let checkbox = snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some("LocalView Toggle"))
            .cloned()
            .expect("real checkbox must be present in the semantic snapshot");
        assert_eq!(
            WindowsUiaActionCapabilities::from_node(&checkbox)
                .support_for(WindowsUiaPattern::Toggle),
            WindowsUiaPatternSupport::Supported,
            "checkbox must advertise Toggle support before planning"
        );
        assert_eq!(
            checkbox.element_ref.acquisition_cut_ref, initial_snapshot_cut,
            "the HTTP Toggle request deliberately starts from the initial cached element ref"
        );

        let postcondition_ref = NativeSemanticPostconditionContractV1 {
            expectation: NativeSemanticPostconditionExpectation::Present,
            matcher: NativeSemanticNodeMatcherV1 {
                name: Some("LocalView Toggle".into()),
                attributes: BTreeMap::from([(
                    TOGGLE_STATE_ATTRIBUTE.into(),
                    "on".into(),
                )]),
                ..Default::default()
            },
        }
        .to_contract_ref()
        .expect("encode typed Toggle-state postcondition contract");

        let journal_path = std::env::temp_dir().join(format!(
            "localview-windows-toggle-control-consequential-{}.jsonl",
            Uuid::new_v4()
        ));
        let journal = Arc::new(
            ConsequentialJournal::open(&journal_path)
                .await
                .expect("open Toggle control-path consequential journal"),
        );
        configure_windows_observe_runtime_for_sessions(&sessions, Some(runtime.clone()));
        configure_windows_consequential_control_for_sessions(&sessions, Some(journal.clone()));

        let app = router(ControlState {
            token: Arc::from("toggle-control-smoke-token"),
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
                        "/v1/sessions/{session_id}/windows-observe/consequential/toggle/plan"
                    ))
                    .header(AUTHORIZATION, "Bearer toggle-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "element_ref": checkbox.element_ref,
                            "expected_postcondition_contract_refs": [postcondition_ref],
                        })
                        .to_string(),
                    ))
                    .expect("build Toggle control-path plan request"),
            )
            .await
            .expect("run Toggle control-path plan request");
        assert_eq!(plan_response.status(), StatusCode::CREATED);
        let plan = response_json(plan_response).await;
        assert_eq!(plan["operation"], "toggle");
        assert_eq!(plan["risk_class"], "s4_destructive_or_irreversible");
        assert_eq!(plan["idempotency_class"], "irreversible");
        let action_id = Uuid::parse_str(
            plan["action_id"]
                .as_str()
                .expect("Toggle plan response must contain action_id"),
        )
        .expect("parse planned Toggle action_id");
        let confirmation_ref = plan["confirmation_ref"]
            .as_str()
            .expect("Toggle plan response must contain confirmation_ref");
        let precondition_cut = plan["precondition_snapshot_cut_ref"]
            .as_str()
            .expect("Toggle plan response must contain precondition cut")
            .to_owned();
        assert_ne!(
            precondition_cut, initial_snapshot_cut,
            "Toggle planning must re-observe the provider instead of admitting the cached initial cut"
        );
        let planning_snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("fresh Toggle plan observation must remain current");
        assert_eq!(
            planning_snapshot.snapshot_cut_ref(),
            precondition_cut,
            "Toggle authority must bind the exact fresh planning cut"
        );
        assert!(
            live.take_public_actions(session_id, 16).await.is_empty(),
            "Toggle consequential actions must never enter the legacy V1-V3 executor queue"
        );

        let confirm_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/{action_id}/confirm"
                    ))
                    .header(AUTHORIZATION, "Bearer toggle-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"confirmation_ref": confirmation_ref}).to_string(),
                    ))
                    .expect("build Toggle control-path confirmation request"),
            )
            .await
            .expect("run Toggle control-path confirmation request");
        assert_eq!(
            confirm_response.status(),
            StatusCode::OK,
            "Toggle must not durably commit until a fresh provider snapshot proves the checkbox is on"
        );
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
            .expect("Toggle commit must retain a postcondition receipt");
        assert_eq!(receipt.verdict, ActionPostconditionVerdict::VerifiedExpected);
        assert_ne!(
            receipt.observation_snapshot_cut_ref, precondition_cut,
            "Toggle verification must use a fresh post-dispatch observation cut"
        );
        let verified_snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("verified Toggle postcondition snapshot must remain current");
        let verified_checkbox = verified_snapshot
            .nodes()
            .iter()
            .find(|node| node.name.as_deref() == Some("LocalView Toggle"))
            .expect("checkbox must remain present after Toggle dispatch");
        assert_eq!(
            verified_checkbox
                .attributes
                .get(TOGGLE_STATE_ATTRIBUTE)
                .map(String::as_str),
            Some("on"),
            "fresh provider evidence must prove the exact retained checkbox is on"
        );

        runtime
            .release(session_id)
            .await
            .expect("release Toggle control-path runtime after verified commit");
        configure_windows_observe_runtime_for_sessions(&sessions, None);
        configure_windows_consequential_control_for_sessions(&sessions, None);
        stop.store(true, Ordering::Release);
        ui_thread
            .join()
            .expect("join Toggle control-path fixture UI thread");

        let _ = std::fs::remove_file(format!(
            "{}.operation-{action_id}.json",
            journal_path.display()
        ));
        let _ = std::fs::remove_file(journal_path);
    }
}
