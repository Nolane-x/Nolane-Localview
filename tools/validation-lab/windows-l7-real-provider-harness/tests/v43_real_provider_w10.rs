#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
mod windows_real_provider_w10 {
    use localview_windows_uia_provider::{WindowsUiaCoordinateSpace, WindowsUiaGeometryRequest};
    use serde_json::{Value, json};

    use super::verified_input_seed::{
        EdgeSeedProcess, attach_and_snapshot, spawn_worker, truth_bool, truth_u64,
    };

    fn truth_i32(value: &Value, field: &str) -> i32 {
        value
            .get(field)
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or_else(|| panic!("W10 geometry oracle field {field} must be i32: {value}"))
    }

    fn assert_geometry_matches_oracle(
        receipt: &localview_windows_uia_provider::WindowsUiaGeometryReceipt,
        truth: &Value,
    ) {
        assert_eq!(
            receipt.coordinate_space,
            WindowsUiaCoordinateSpace::PhysicalScreenPixels,
            "W10 shipping geometry must remain explicitly physical-screen-pixel authority"
        );
        assert_eq!(receipt.target_window_dpi, truth_u64(truth, "window_dpi") as u32);
        assert_eq!(receipt.bounding_rect.left, truth_i32(truth, "uia_left"));
        assert_eq!(receipt.bounding_rect.top, truth_i32(truth, "uia_top"));
        assert_eq!(receipt.bounding_rect.right, truth_i32(truth, "uia_right"));
        assert_eq!(receipt.bounding_rect.bottom, truth_i32(truth, "uia_bottom"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real interactive Windows UI Automation provider and two displays with distinct effective DPI"]
    async fn w10_mixed_dpi_move_refreshes_live_physical_geometry_without_stale_coordinates() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider seed execution must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let fixture = seed.prepare_input_target();
        let target_window = truth_u64(&fixture, "window_handle");
        assert_ne!(target_window, 0, "W10 target HWND must be real");

        let before_truth = seed.command(json!({ "command": "get_geometry_state" }));
        assert_eq!(
            before_truth.get("ok").and_then(Value::as_bool),
            Some(true),
            "W10 edge seed must expose an independent geometry oracle: {before_truth}"
        );
        assert!(
            truth_bool(&before_truth, "mixed_dpi_capable"),
            "W10 requires at least two real displays with distinct effective DPI; this environment must not mint a false mixed-DPI pass: {before_truth}"
        );

        let worker = spawn_worker();
        let (attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w10:before-mixed-dpi-move",
        );
        let root = snapshot
            .nodes()
            .first()
            .expect("W10 production UIA snapshot must retain the target window root");
        let request = WindowsUiaGeometryRequest::new(
            snapshot.snapshot_cut_ref(),
            root.element_ref.clone(),
        )
        .expect("W10 root geometry request must bind the exact acquisition cut");

        let before = worker
            .observe_geometry(&attachment, request.clone())
            .expect("W10 must observe initial live geometry through the shipping worker");
        assert_geometry_matches_oracle(&before, &before_truth);

        let after_truth = seed.command(json!({ "command": "move_to_alternate_dpi_monitor" }));
        assert_eq!(
            after_truth.get("ok").and_then(Value::as_bool),
            Some(true),
            "W10 seed must perform a real HWND move onto a display with different effective DPI: {after_truth}"
        );
        assert_eq!(truth_u64(&after_truth, "window_handle"), target_window);
        assert_ne!(
            truth_u64(&after_truth, "window_dpi"),
            truth_u64(&before_truth, "window_dpi"),
            "independent oracle must prove the same HWND crossed an actual DPI boundary"
        );

        let after = worker
            .observe_geometry(&attachment, request)
            .expect("W10 retained live element must refresh geometry after cross-DPI movement");
        assert_geometry_matches_oracle(&after, &after_truth);
        assert_ne!(
            after.bounding_rect, before.bounding_rect,
            "W10 must never replay the pre-move physical rectangle as current geometry"
        );
        assert_ne!(
            after.target_window_dpi, before.target_window_dpi,
            "W10 target-window DPI authority must refresh after crossing the monitor DPI boundary"
        );

        seed.shutdown();
    }
}
