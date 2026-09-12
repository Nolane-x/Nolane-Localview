#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
mod windows_real_provider_w11 {
    use localview_windows_uia_provider::{
        WindowsUiaDispatchContextBlocker, WindowsUiaWorkerError,
    };

    use super::verified_input_seed::{
        EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,
        mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w11_modal_before_dispatch_is_blocked_before_any_real_input_effect() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let fixture = seed.prepare_input_target();
        let target_window = truth_u64(&fixture, "window_handle");
        assert_ne!(target_window, 0, "W11 target HWND must be real");
        assert!(
            truth_bool(&fixture, "target_is_foreground"),
            "W11 authority must be minted while the exact target is foreground"
        );
        assert_eq!(truth_u64(&fixture, "effect_count"), 0);

        let worker = spawn_worker();
        let (attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w11:before-modal",
        );
        let target = snapshot
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
            .expect("production UIA snapshot must retain the deterministic W11 input target");
        let authority = mint_verified_input_authority(
            &attachment,
            snapshot.as_ref(),
            target.element_ref.clone(),
        )
        .await;

        let modal = seed.open_modal_blocker();
        let modal_window = truth_u64(&modal, "modal_window_handle");
        assert_ne!(modal_window, 0, "W11 modal HWND must be real");
        assert_ne!(modal_window, target_window, "W11 modal must be a distinct HWND");
        assert!(truth_bool(&modal, "modal_is_open"));
        assert_eq!(
            truth_u64(&modal, "modal_owner_window_handle"),
            target_window,
            "independent oracle must prove the blocker is owned by the authorized target"
        );
        assert_eq!(truth_u64(&modal, "effect_count"), 0);

        let error = worker
            .dispatch_verified_input(&attachment, authority.request)
            .expect_err("W11 owned modal must fail closed before SendInput");
        assert_eq!(
            error,
            WindowsUiaWorkerError::DispatchContextBlocked(
                WindowsUiaDispatchContextBlocker::ModalBlockerPresent {
                    window_handle: modal_window,
                }
            ),
            "the immediate worker-owned context fence must bind the exact owned modal HWND"
        );

        let after = seed.input_state();
        assert_eq!(
            truth_u64(&after, "effect_count"),
            0,
            "W11 independent target oracle must observe zero keyboard effect after modal block"
        );
        assert!(truth_bool(&after, "modal_is_open"));

        seed.close_modal_blocker();
        authority
            .journal
            .abandon_dispatch_execution(authority.permit)
            .await
            .expect("W11 blocked attempt must consume only volatile execution authority");
        drop(authority.journal);
        let _ = std::fs::remove_file(authority.journal_path);
        seed.shutdown();
    }
}
