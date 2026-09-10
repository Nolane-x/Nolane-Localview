#[cfg(windows)]
mod windows_consequential_set_value_windows_smoke {
    use std::{
        fs,
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
        CanonicalActionOperation, ConsequentialJournal, LiveBridge, SetValueMode,
    };
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_observation::ObservationBus;
    use localview_protocol::{
        Classification, DiscoveredServer, Endpoint, ListenerCandidate, ServerKind,
    };
    use localview_sessions::SessionManager;
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, spawn_windows_uia_runtime_manager,
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
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
                PeekMessageW, SW_SHOW, ShowWindow, TranslateMessage, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const SENTINEL: &str = "LocalView Task8 HTTP SetValue secret 4f68c5d2";

    fn discovered() -> DiscoveredServer {
        DiscoveredServer {
            candidate: ListenerCandidate {
                endpoint: Endpoint {
                    host: "127.0.0.1".into(),
                    port: 5914,
                    scheme: "http".into(),
                },
                pid: Some(94),
                process_name: Some("localview-set-value-plan-smoke".into()),
                command: Some("localview-set-value-plan-smoke".into()),
                cwd: Some("C:/localview-set-value-plan-smoke".into()),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("LocalViewSetValuePlanSmoke".into()),
                title: None,
                hmr_detected: false,
                evidence: Default::default(),
            },
        }
    }

    async fn response_body(response: axum::response::Response) -> (StatusCode, String) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("read bounded SetValue plan response");
        let body = String::from_utf8(bytes.to_vec()).expect("SetValue plan response must be UTF-8");
        (status, body)
    }

    fn assert_artifact_private(bytes: &[u8], label: &str) {
        assert!(
            !bytes.windows(SENTINEL.len()).any(|window| window == SENTINEL.as_bytes()),
            "{label} must never persist SetValue plaintext"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_http_set_value_plan_persists_only_opaque_server_owned_authority() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-set-value-control-plan-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        w!("LocalView SetValue HTTP Before"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        520,
                        180,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create deterministic editable SetValue HTTP fixture")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish SetValue HTTP fixture HWND");

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
                    DestroyWindow(window).expect("destroy SetValue HTTP fixture");
                }
            })
            .expect("spawn responsive SetValue HTTP fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive SetValue HTTP fixture HWND");

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
            .expect("spawn concrete Windows UIA SetValue control runtime"),
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
            .expect("attach exact editable Win32 target");

        let snapshot = runtime
            .current_semantic_snapshot(session_id)
            .await
            .expect("attached runtime must expose initial semantic snapshot");
        let initial_snapshot_cut = snapshot.snapshot_cut_ref().to_owned();
        let value_node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                WindowsUiaActionCapabilities::from_node(node).support_for(WindowsUiaPattern::Value)
                    == WindowsUiaPatternSupport::Supported
            })
            .cloned()
            .expect("editable Win32 fixture must advertise UIA Value support");

        let journal_path = std::env::temp_dir().join(format!(
            "localview-windows-control-set-value-plan-{}.jsonl",
            Uuid::new_v4()
        ));
        let journal = Arc::new(
            ConsequentialJournal::open(&journal_path)
                .await
                .expect("open SetValue control journal"),
        );
        configure_windows_observe_runtime_for_sessions(&sessions, Some(runtime.clone()));
        configure_windows_consequential_control_for_sessions(&sessions, Some(journal.clone()));

        let app = router(ControlState {
            token: Arc::from("set-value-control-smoke-token"),
            sessions: sessions.clone(),
            observations: ObservationBus::new(32),
            live: live.clone(),
            evidence: EvidenceStore::default(),
            paused: Arc::new(AtomicBool::new(false)),
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/v1/sessions/{session_id}/windows-observe/consequential/set-value/plan"
                    ))
                    .header(AUTHORIZATION, "Bearer set-value-control-smoke-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "mode": "replace_value",
                            "element_ref": value_node.element_ref,
                            "value": SENTINEL,
                        })
                        .to_string(),
                    ))
                    .expect("build SetValue plan request"),
            )
            .await
            .expect("run SetValue plan request");
        let (status, body) = response_body(response).await;
        assert!(!body.contains(SENTINEL), "fail-closed response must remain private");
        assert_eq!(
            status,
            StatusCode::CREATED,
            "SetValue planning failed closed at: {body}"
        );
        assert!(!body.contains("commitment_digest"));

        let plan: serde_json::Value = serde_json::from_str(&body).expect("decode SetValue plan metadata");
        assert_eq!(plan["operation"], "set_value");
        assert_eq!(plan["risk_class"], "s4_destructive_or_irreversible");
        assert_eq!(plan["idempotency_class"], "irreversible");
        assert_eq!(plan["confirmation_required"], true);
        assert_eq!(plan["restart_restores_confirmation_authority"], false);
        let action_id = Uuid::parse_str(
            plan["action_id"]
                .as_str()
                .expect("SetValue plan response must contain action_id"),
        )
        .expect("parse SetValue action id");
        assert!(plan["confirmation_ref"].as_str().is_some());
        let precondition_cut = plan["precondition_snapshot_cut_ref"]
            .as_str()
            .expect("SetValue plan response must contain fresh precondition cut");
        assert_ne!(precondition_cut, initial_snapshot_cut);

        assert_eq!(
            journal.admitted_operation(action_id).await.unwrap(),
            Some(CanonicalActionOperation::SetValue)
        );
        let binding = journal
            .set_value_payload_binding(action_id)
            .await
            .unwrap()
            .expect("SetValue plan must durably persist opaque payload binding before confirmation");
        assert_eq!(binding.action_id, action_id);
        assert_eq!(binding.mode, SetValueMode::ReplaceValue);
        assert_eq!(binding.payload_utf8_len, SENTINEL.len() as u64);
        assert_eq!(binding.commitment_digest.len(), 32);
        assert!(
            live.take_public_actions(session_id, 16).await.is_empty(),
            "SetValue plan must not enter the legacy executor queue"
        );

        let journal_bytes = fs::read(&journal_path).expect("read SetValue journal artifact");
        assert_artifact_private(&journal_bytes, "journal");
        let operation_path = std::path::PathBuf::from(format!(
            "{}.operation-{action_id}.json",
            journal_path.display()
        ));
        let payload_path = std::path::PathBuf::from(format!(
            "{}.set-value-payload-{action_id}.json",
            journal_path.display()
        ));
        assert_artifact_private(
            &fs::read(&operation_path).expect("read operation companion"),
            "operation companion",
        );
        assert_artifact_private(
            &fs::read(&payload_path).expect("read SetValue payload companion"),
            "payload companion",
        );

        configure_windows_consequential_control_for_sessions(&sessions, None);
        runtime
            .release(session_id)
            .await
            .expect("release SetValue control runtime");
        configure_windows_observe_runtime_for_sessions(&sessions, None);
        stop.store(true, Ordering::Release);
        ui_thread.join().expect("join SetValue HTTP fixture UI thread");

        let _ = fs::remove_file(operation_path);
        let _ = fs::remove_file(payload_path);
        let _ = fs::remove_file(journal_path);
    }
}
