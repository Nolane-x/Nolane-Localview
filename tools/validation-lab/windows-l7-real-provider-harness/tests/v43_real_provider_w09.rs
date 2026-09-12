#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
mod windows_real_provider_w09 {
    use localview_windows_uia_provider::{
        WindowsUiaWorkerError, WindowsVerifiedInputBoundaryError,
    };

    use super::verified_input_seed::{
        EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,
        mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    async fn w09_user_held_modifier_is_blocked_without_normalization_or_input_effect() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let fixture = seed.prepare_input_target();
        let target_window = truth_u64(&fixture, "window_handle");
        assert!(
            truth_bool(&fixture, "target_is_foreground"),
            "W09 must start with the exact target foreground before establishing modifier conflict"
        );
        assert_eq!(truth_u64(&fixture, "effect_count"), 0);

        let worker = spawn_worker();
        let (attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w09:before-modifier-conflict",
        );
        let target = snapshot
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
            .expect("production UIA snapshot must retain the deterministic W09 input target");
        let authority = mint_verified_input_authority(
            &attachment,
            snapshot.as_ref(),
            target.element_ref.clone(),
        )
        .await;

        let held = seed.hold_shift();
        assert!(truth_bool(&held, "shift_down"));
        assert_eq!(truth_u64(&held, "effect_count"), 0);

        let error = worker
            .dispatch_verified_input(&attachment, authority.request)
            .expect_err("W09 conflicting Shift state must fail closed before SendInput");
        assert_eq!(
            error,
            WindowsUiaWorkerError::VerifiedInputBoundary(
                WindowsVerifiedInputBoundaryError::InputStateConflict
            ),
            "production boundary must report typed InputStateConflict"
        );

        let after = seed.input_state();
        assert!(
            truth_bool(&after, "shift_down"),
            "LocalView must not synthesize a Shift release to normalize user/test-owned input state"
        );
        assert_eq!(
            truth_u64(&after, "effect_count"),
            0,
            "W09 independent target oracle must observe zero keyboard effect after modifier block"
        );

        seed.release_shift();
        authority
            .journal
            .abandon_dispatch_execution(authority.permit)
            .await
            .expect("W09 blocked attempt must consume only volatile execution authority");
        drop(authority.journal);
        let _ = std::fs::remove_file(authority.journal_path);
        seed.shutdown();
    }
}
