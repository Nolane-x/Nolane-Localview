#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
mod windows_real_provider_w12 {
    use localview_windows_uia_provider::WindowsUiaWorkerError;

    use super::verified_input_seed::{
        EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, abandon_authority, attach_and_snapshot,
        mint_verified_input_authority, spawn_worker, truth_u64,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w12_restart_rejects_pre_restart_authority_before_any_replacement_effect() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed_a = EdgeSeedProcess::spawn();
        let fixture_a = seed_a.prepare_input_target();
        let a_window = truth_u64(&fixture_a, "window_handle");
        let a_process = seed_a.process_id();
        assert_ne!(a_window, 0, "W12 process A HWND must be real");
        assert_eq!(truth_u64(&fixture_a, "effect_count"), 0);

        let worker = spawn_worker();
        let (attachment_a, snapshot_a) = attach_and_snapshot(
            &worker,
            &seed_a,
            a_window,
            "cut:v43:w12:process-a",
        );
        let target_a = snapshot_a
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
            .expect("production UIA snapshot must retain the deterministic W12 process-A target");
        let authority_a = mint_verified_input_authority(
            &attachment_a,
            snapshot_a.as_ref(),
            target_a.element_ref.clone(),
        )
        .await;
        let a_target_incarnation = attachment_a.target_incarnation_ref().clone();
        let a_fingerprint = attachment_a.fingerprint().clone();

        seed_a.kill_and_wait();

        let mut seed_b = EdgeSeedProcess::spawn();
        let fixture_b = seed_b.prepare_input_target();
        let b_window = truth_u64(&fixture_b, "window_handle");
        let b_process = seed_b.process_id();
        assert_ne!(b_window, 0, "W12 replacement HWND must be real");
        assert_eq!(truth_u64(&fixture_b, "effect_count"), 0);

        let stale_error = worker
            .dispatch_verified_input(&attachment_a, authority_a.request)
            .expect_err("W12 pre-restart authority must not dispatch after process A is gone");
        assert_eq!(
            stale_error,
            WindowsUiaWorkerError::TargetReincarnated,
            "target death/restart must be typed as stale target authority before SendInput"
        );

        let after_stale = seed_b.input_state();
        assert_eq!(
            truth_u64(&after_stale, "effect_count"),
            0,
            "W12 replacement process B must observe zero effect from process-A authority"
        );

        let (attachment_b, snapshot_b) = attach_and_snapshot(
            &worker,
            &seed_b,
            b_window,
            "cut:v43:w12:process-b",
        );
        assert_ne!(
            attachment_b.fingerprint(),
            &a_fingerprint,
            "W12 replacement must have fresh provider-observed target lifetime facts; A pid={a_process}, B pid={b_process}"
        );
        assert_ne!(
            attachment_b.target_incarnation_ref(),
            &a_target_incarnation,
            "W12 fresh reacquire must mint a distinct target incarnation"
        );
        let target_b = snapshot_b
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
            .expect("production UIA snapshot must reacquire the same logical target in process B");
        let authority_b = mint_verified_input_authority(
            &attachment_b,
            snapshot_b.as_ref(),
            target_b.element_ref.clone(),
        )
        .await;
        abandon_authority(authority_b).await;

        authority_a
            .journal
            .abandon_dispatch_execution(authority_a.permit)
            .await
            .expect("W12 stale attempt must consume only volatile process-A execution authority");
        drop(authority_a.journal);
        let _ = std::fs::remove_file(authority_a.journal_path);
        seed_b.shutdown();
    }
}
