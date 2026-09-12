#[cfg(windows)]
mod windows_real_provider_w07 {
    #[path = "support/v43_verified_input_seed.rs"]
    mod support;

    use localview_windows_uia_provider::{
        WindowsUiaDispatchContextBlocker, WindowsUiaWorkerError,
    };
    use support::{
        EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,
        mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w07_foreground_theft_is_blocked_before_any_real_input_effect() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let fixture = seed.prepare_input_target();
        let target_window = truth_u64(&fixture, "window_handle");
        assert_ne!(target_window, 0, "W07 target HWND must be real");
        assert!(
            truth_bool(&fixture, "target_is_foreground"),
            "W07 authorization must begin while the exact target is foreground"
        );
        assert_eq!(truth_u64(&fixture, "effect_count"), 0);

        let worker = spawn_worker();
        let (attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w07:before-foreground-theft",
        );
        let target = snapshot
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
            .expect("production UIA snapshot must retain the deterministic W07 input target");
        let authority = mint_verified_input_authority(&attachment, snapshot.as_ref(), target.element_ref.clone()).await;

        let stolen = seed.steal_foreground();
        let thief_window = truth_u64(&stolen, "thief_window_handle");
        assert_ne!(thief_window, target_window, "W07 thief must be a distinct real HWND");
        assert_eq!(
            truth_u64(&stolen, "foreground_window_handle"),
            thief_window,
            "independent oracle must prove theft before final LocalView boundary"
        );

        let error = worker
            .dispatch_verified_input(&attachment, authority.request)
            .expect_err("W07 foreground theft must fail closed before SendInput");
        assert_eq!(
            error,
            WindowsUiaWorkerError::DispatchContextBlocked(
                WindowsUiaDispatchContextBlocker::ForegroundWindowMismatch {
                    expected: target_window,
                    actual: thief_window,
                }
            ),
            "the final MTA fence must reject the stolen foreground HWND, not infer later success"
        );

        let after = seed.input_state();
        assert_eq!(
            truth_u64(&after, "effect_count"),
            0,
            "W07 independent target oracle must observe zero keyboard effect after blocked dispatch"
        );
        assert!(
            !truth_bool(&after, "target_is_foreground"),
            "the blocked LocalView attempt must not steal foreground back from the test-owned thief"
        );

        authority
            .journal
            .abandon_dispatch_execution(authority.permit)
            .await
            .expect("W07 blocked attempt must consume only volatile execution authority");
        drop(authority.journal);
        let _ = std::fs::remove_file(authority.journal_path);
        seed.shutdown();
    }
}
