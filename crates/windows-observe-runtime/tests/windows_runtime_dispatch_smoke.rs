#[cfg(windows)]
mod windows_runtime_dispatch_smoke {
    use std::{
        convert::Infallible,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::Duration,
    };

    use localview_live_bridge::{
        ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, BridgeActionKind,
        ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,
        ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,
        ConsequentialRecoveryState, LiveBridge, reconcile_consequential_postconditions,
    };
    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};
    use localview_protocol::{DispatchResult, PrincipalRef, TransportResult, WorldOutcome};
    use localview_windows_observe_runtime::{
        WindowsObserveRuntimeConfig, WindowsUiaActionPreflightRequest,
        WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,
        WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchRequest,
        arm_uia_dispatch_execution, execute_armed_uia_dispatch, prepare_uia_dispatch,
        spawn_windows_uia_runtime_manager,
    };
    use localview_windows_uia_provider::{
        WindowsUiaActionCapabilities, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,
        WindowsUiaPatternSupport, WindowsUiaWorkerConfig,
    };
    use uuid::Uuid;
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, WPARAM},
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GWLP_WNDPROC, MSG, PM_REMOVE, PeekMessageW, SW_SHOW, SetWindowLongPtrW,
                SetWindowTextW, ShowWindow, TranslateMessage, WM_COMMAND, WS_CHILD,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::w,
    };

    const BEFORE_TITLE: &str = "LocalView Runtime Before";
    const AFTER_TITLE: &str = "LocalView Runtime Invoked";
    const POSTCONDITION_REF: &str = "postcondition:runtime-smoke";

    unsafe extern "system" fn smoke_parent_wndproc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_COMMAND {
            unsafe {
                let _ = SetWindowTextW(window, w!("LocalView Runtime Invoked"));
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    struct SmokeAuthorizationRevalidator;

    impl WindowsUiaAuthorizationRevalidator for SmokeAuthorizationRevalidator {
        type Error = Infallible;

        fn revalidate(
            &self,
            action_id: Uuid,
            authority: &ActionEnvelopeMetadata,
        ) -> Result<WindowsUiaAuthorizationRevalidationReceipt, Self::Error> {
            Ok(WindowsUiaAuthorizationRevalidationReceipt {
                action_id,
                decision_principal_ref: authority.decision_principal_ref.clone(),
                acting_principal_ref: authority.acting_principal_ref.clone(),
                authorization_revision: authority.authorization_revision.clone(),
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider"]
    async fn real_runtime_invoke_is_verified_by_fresh_post_dispatch_snapshot_before_commit() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real UIA smoke must be explicitly enabled"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let ui_stop = Arc::clone(&stop);
        let (window_tx, window_rx) = mpsc::sync_channel(1);
        let ui_thread = thread::Builder::new()
            .name("localview-uia-runtime-postcondition-smoke-ui".into())
            .spawn(move || {
                let window = unsafe {
                    CreateWindowExW(
                        Default::default(),
                        w!("STATIC"),
                        w!("LocalView Runtime Before"),
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        460,
                        220,
                        None,
                        None,
                        None,
                        None,
                    )
                    .expect("create Win32 runtime postcondition parent fixture")
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
                        w!("Invoke Postcondition"),
                        WS_CHILD | WS_VISIBLE,
                        36,
                        52,
                        220,
                        52,
                        Some(window),
                        None,
                        None,
                        None,
                    )
                    .expect("create child Win32 Invoke button")
                };
                unsafe {
                    let _ = ShowWindow(window, SW_SHOW);
                }
                window_tx
                    .send(window.0 as usize as u64)
                    .expect("publish runtime postcondition fixture HWND");

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
                    DestroyWindow(window).expect("destroy Win32 runtime postcondition fixture");
                }
            })
            .expect("spawn responsive Win32 runtime postcondition fixture thread");

        let window_handle = window_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive runtime postcondition fixture HWND");
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
        .expect("spawn concrete Windows UIA runtime");

        let session_id = Uuid::new_v4();
        manager
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

        let snapshot = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("attached runtime must expose its immutable current semantic revision");
        assert!(
            snapshot
                .nodes()
                .iter()
                .any(|node| node.name.as_deref() == Some(BEFORE_TITLE)),
            "initial UIA snapshot must observe the pre-dispatch world state"
        );
        let node = snapshot
            .nodes()
            .iter()
            .find(|node| {
                WindowsUiaActionCapabilities::from_node(node).support_for(WindowsUiaPattern::Invoke)
                    == WindowsUiaPatternSupport::Supported
            })
            .cloned()
            .expect("real child BUTTON snapshot must advertise Invoke support");

        let authority = ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:runtime-smoke"),
            acting_principal_ref: PrincipalRef::from("principal:acting:runtime-smoke"),
            authorization_revision: "authorization:runtime-smoke:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: snapshot.provider_incarnation_ref().clone(),
            target_incarnation_ref: snapshot.target_incarnation_ref().clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec![POSTCONDITION_REF.into()],
        };
        let queued = bridge
            .enqueue_canonical_action(session_id, None, BridgeActionKind::Focus, authority.clone())
            .await
            .expect("enqueue canonical consequential action");

        let journal_path = std::env::temp_dir().join(format!(
            "localview-windows-runtime-postcondition-{}.jsonl",
            Uuid::new_v4()
        ));
        let journal = ConsequentialJournal::open(&journal_path)
            .await
            .expect("open consequential journal");
        journal
            .record_intent_admitted(queued.envelope.clone())
            .await
            .expect("durably admit canonical action intent");

        let preflight = manager
            .preflight_uia_action(
                session_id,
                WindowsUiaActionPreflightRequest {
                    authority: authority.clone(),
                    element_ref: node.element_ref.clone(),
                    required_pattern: WindowsUiaPattern::Invoke,
                },
            )
            .await
            .expect("preflight exact current Invoke node");
        let prepared = prepare_uia_dispatch(
            &bridge,
            &journal,
            &manager,
            session_id,
            WindowsUiaPreparedDispatchRequest {
                seal: WindowsUiaDispatchSealRequest {
                    action_id: queued.action.id,
                    authority,
                    preflight,
                    context_requirements: WindowsUiaDispatchContextRequirements {
                        require_foreground_target: false,
                        require_exact_element_focus: false,
                        require_no_modal_blocker: true,
                    },
                },
            },
            &SmokeAuthorizationRevalidator,
        )
        .await
        .expect("durably prepare real Windows UIA dispatch");
        let armed = arm_uia_dispatch_execution(&bridge, &journal, &manager, session_id, prepared)
            .await
            .expect("arm exactly one provider execution request");
        let action_id = armed.action_id();
        let executor = manager
            .uia_dispatch_executor(session_id)
            .await
            .expect("resolve exact attached runtime dispatch executor");

        let result = execute_armed_uia_dispatch(&bridge, &journal, session_id, armed, &executor)
            .await
            .expect("real retained UIA Invoke must cross the one-shot runtime boundary");

        assert_eq!(
            result.provider_receipt.transport_result,
            TransportResult::DeliveredToExecutor
        );
        assert_eq!(
            result.provider_receipt.dispatch_result,
            DispatchResult::DispatchedFull
        );
        assert_eq!(
            journal.recovery_state(action_id).await,
            Some(ConsequentialRecoveryState::PossiblyDispatched),
            "provider success is dispatch evidence only until postcondition reconciliation"
        );
        assert!(matches!(
            result.journal_entry.transition,
            ConsequentialJournalTransition::DispatchLinearized { .. }
        ));

        tokio::time::sleep(Duration::from_millis(50)).await;
        let observation_permit = journal
            .begin_postcondition_observation(action_id)
            .await
            .expect("durable dispatch uncertainty must mint one post-dispatch observation permit");
        let observation = manager
            .capture_postcondition_observation(&journal, observation_permit)
            .await
            .expect("runtime must capture the exact journal-authorized fresh UIA snapshot");
        let post_snapshot = manager
            .current_semantic_snapshot(session_id)
            .await
            .expect("runtime must retain the post-dispatch semantic revision");
        assert_eq!(
            post_snapshot.snapshot_cut_ref(),
            observation.snapshot_cut_ref()
        );
        assert_ne!(
            post_snapshot.snapshot_cut_ref(),
            snapshot.snapshot_cut_ref(),
            "postcondition evidence must use a fresh cut after dispatch"
        );

        let title_changed = post_snapshot
            .nodes()
            .iter()
            .any(|node| node.name.as_deref() == Some(AFTER_TITLE));
        let reconciliation = reconcile_consequential_postconditions(
            &bridge,
            &journal,
            ConsequentialPostconditionReconciliationReceipt::from_observation(
                observation,
                vec![ConsequentialPostconditionEvidence {
                    contract_ref: POSTCONDITION_REF.into(),
                    status: if title_changed {
                        ConsequentialPostconditionStatus::VerifiedPass
                    } else {
                        ConsequentialPostconditionStatus::VerifiedFail
                    },
                    receipt_ref: format!(
                        "postcondition:runtime-smoke:{}",
                        post_snapshot.cache_revision_ref()
                    ),
                }],
            ),
        )
        .await
        .expect("typed postcondition reconciliation must consume causal snapshot evidence");

        assert_eq!(reconciliation.world_outcome, WorldOutcome::VerifiedExpected);
        assert!(reconciliation.postconditions_verified);
        assert_eq!(
            journal.recovery_state(action_id).await,
            Some(ConsequentialRecoveryState::VerifiedUncommitted),
            "only the observed expected world state may cross the verified boundary"
        );
        journal
            .record_committed(action_id)
            .await
            .expect("verified expected postconditions may commit");
        assert_eq!(
            journal.recovery_state(action_id).await,
            Some(ConsequentialRecoveryState::Committed)
        );

        manager
            .release(session_id)
            .await
            .expect("release runtime after verified postcondition commit");
        stop.store(true, Ordering::Release);
        ui_thread
            .join()
            .expect("join runtime postcondition fixture UI thread");
        let _ = std::fs::remove_file(journal_path);
    }
}
