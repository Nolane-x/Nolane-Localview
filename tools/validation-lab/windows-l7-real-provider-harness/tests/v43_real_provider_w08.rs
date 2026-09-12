#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
#[path = "support/v43_verified_input_full_smoke.rs"]
mod verified_input_full_smoke;

#[cfg(windows)]
mod windows_real_provider_w08 {
    use std::{fs, path::PathBuf};

    use localview_protocol::DispatchResult;
    use localview_windows_observe_runtime::dispatch_result_for_verified_input;
    use localview_windows_uia_provider::{
        WindowsInputInsertRawResult, WindowsInputInsertionClass, WindowsKeyTransition,
        WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextObservation,
        WindowsUiaDispatchContextRequirements, WindowsVerifiedInputBoundaryError,
        WindowsVerifiedInputEnvironment, WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch,
        execute_windows_verified_input_boundary,
    };
    use serde_json::json;

    const TARGET_HWND: u64 = 0x4308;
    const TARGET_PID: u32 = 4308;

    #[derive(Debug, Default)]
    struct DeterministicPartialInserter {
        insert_calls: u32,
    }

    impl WindowsVerifiedInputEnvironment for DeterministicPartialInserter {
        fn observe_dispatch_context(
            &mut self,
        ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError>
        {
            Ok(WindowsUiaDispatchContextObservation {
                target_window_handle: TARGET_HWND,
                target_process_id: TARGET_PID,
                foreground_window_handle: Some(TARGET_HWND),
                foreground_process_id: Some(TARGET_PID),
                exact_element_focused: None,
                modal_blocker_window_handle: None,
            })
        }

        fn snapshot_keyboard_state(
            &mut self,
        ) -> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError> {
            Ok(WindowsKeyboardStateSnapshot {
                shift_down: false,
                control_down: false,
                alt_down: false,
                left_windows_down: false,
                right_windows_down: false,
                caps_lock_on: false,
                num_lock_on: false,
                scroll_lock_on: false,
                layout_identity: Some("deterministic-wrapper-layout".into()),
            })
        }

        fn insert_events(
            &mut self,
            events: &[WindowsVerifiedKeyEvent],
        ) -> WindowsInputInsertRawResult {
            self.insert_calls += 1;
            assert_eq!(
                events.len(),
                4,
                "W08 wrapper must receive the exact authorized batch"
            );
            WindowsInputInsertRawResult {
                requested_event_count: 4,
                inserted_event_count: 2,
                raw_error_code: None,
            }
        }
    }

    fn four_event_batch() -> WindowsVerifiedKeyboardBatch {
        WindowsVerifiedKeyboardBatch::new(vec![
            WindowsVerifiedKeyEvent {
                virtual_key: 0x20,
                transition: WindowsKeyTransition::KeyDown,
            },
            WindowsVerifiedKeyEvent {
                virtual_key: 0x20,
                transition: WindowsKeyTransition::KeyUp,
            },
            WindowsVerifiedKeyEvent {
                virtual_key: 0x0D,
                transition: WindowsKeyTransition::KeyDown,
            },
            WindowsVerifiedKeyEvent {
                virtual_key: 0x0D,
                transition: WindowsKeyTransition::KeyUp,
            },
        ])
        .expect("construct bounded deterministic W08 batch")
    }

    fn artifact_dir() -> Option<PathBuf> {
        std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR").map(PathBuf::from)
    }

    #[test]
    fn w08_deterministic_partial_wrapper_preserves_unknown_without_retry() {
        let mut environment = DeterministicPartialInserter::default();
        let receipt = execute_windows_verified_input_boundary(
            WindowsUiaDispatchContextRequirements {
                require_foreground_target: true,
                require_exact_element_focus: false,
                require_no_modal_blocker: true,
            },
            &four_event_batch(),
            &mut environment,
        )
        .expect("deterministic W08 wrapper must reach production insertion classification");

        assert_eq!(receipt.requested_event_count, 4);
        assert_eq!(receipt.inserted_event_count, 2);
        assert_eq!(
            receipt.insertion_class,
            WindowsInputInsertionClass::PartialDispatchUnknownOutcome
        );
        assert!(
            receipt.reconciliation_required,
            "partial insertion must require post-dispatch reconciliation"
        );
        assert_eq!(
            dispatch_result_for_verified_input(&receipt),
            DispatchResult::DispatchedPartial,
            "runtime journal mapping must preserve partial external side effect"
        );
        assert_eq!(
            environment.insert_calls, 1,
            "the production boundary must never blind-retry a partial insertion"
        );

        if let Some(path) = artifact_dir() {
            fs::create_dir_all(&path).expect("create bounded W08 artifact directory");
            let artifact = json!({
                "case_id": "W08-partial-input-dispatch",
                "evidence_kind": "deterministic-partial-wrapper",
                "production_boundary_path": "execute_windows_verified_input_boundary",
                "requested_event_count": receipt.requested_event_count,
                "inserted_event_count": receipt.inserted_event_count,
                "insertion_class": "partial-dispatch-unknown-outcome",
                "reconciliation_required": receipt.reconciliation_required,
                "blind_retry_authorized": false,
                "inserter_call_count": environment.insert_calls,
                "natural_windows_partial_observed": false,
                "claim_boundary": "wrapper/property evidence only; does not claim hosted Windows naturally produced a partial SendInput result"
            });
            fs::write(
                path.join("W08-DETERMINISTIC-PARTIAL-WRAPPER.json"),
                serde_json::to_vec_pretty(&artifact).expect("serialize W08 wrapper artifact"),
            )
            .expect("persist W08 wrapper artifact");
        }
    }
}

#[cfg(windows)]
mod windows_real_provider_w08_full {
    use std::{fs, path::PathBuf};

    use localview_protocol::DispatchResult;
    use localview_windows_uia_provider::WindowsInputInsertionClass;
    use serde_json::json;

    use super::{
        verified_input_full_smoke::run_production_verified_input_full_smoke,
        verified_input_seed::EdgeSeedProcess,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires interactive Windows desktop and WPF edge seed"]
    async fn w08_production_worker_full_dispatch_uses_authority_path() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real production input smoke must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let full = run_production_verified_input_full_smoke(
            &mut seed,
            "cut:v43:w08:production-worker-full",
        )
        .await;
        assert_eq!(full.requested_event_count, 2);
        assert_eq!(full.inserted_event_count, 2);
        assert_eq!(
            full.insertion_class,
            WindowsInputInsertionClass::FullyInserted
        );
        assert!(full.reconciliation_required);
        assert_eq!(full.dispatch_result, DispatchResult::DispatchedFull);
        assert_eq!(full.effect_count, 1);
        assert!(full.target_is_foreground);

        if let Some(dir) = std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR") {
            let path = PathBuf::from(dir).join("W08-PRODUCTION-FULL-SMOKE.json");
            let artifact = json!({
                "case_id": "W08-partial-input-dispatch",
                "evidence_kind": "production-windows-worker-full-dispatch-smoke",
                "candidate_sha": std::env::var("LOCALVIEW_CANDIDATE_SHA")
                    .unwrap_or_else(|_| "unknown:standalone-w08-smoke".into()),
                "requested_event_count": full.requested_event_count,
                "inserted_event_count": full.inserted_event_count,
                "insertion_class": "fully-inserted",
                "dispatch_result": "dispatched-full",
                "reconciliation_required": full.reconciliation_required,
                "authority_path": "journal-minted WindowsUiaVerifiedInputRequest -> WindowsUiaWorker::dispatch_verified_input",
                "independent_oracle_effect_count": full.effect_count,
                "natural_windows_partial_observed": false,
                "claim_boundary": "production backend evidence proves a full dispatch only through the authority-gated worker; it does not claim hosted Windows naturally produced a partial SendInput result"
            });
            fs::create_dir_all(path.parent().expect("artifact parent"))
                .expect("create W08 artifact directory");
            fs::write(
                path,
                serde_json::to_vec_pretty(&artifact)
                    .expect("serialize W08 production worker smoke artifact"),
            )
            .expect("persist W08 production worker smoke artifact");
        }

        seed.shutdown();
    }
}
