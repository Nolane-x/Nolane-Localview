use std::collections::BTreeSet;

use localview_validation_lab::{
    RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth, RealProviderLabRecord,
    RealProviderObservedOutcome, adapt_real_provider_case, canonical_digest,
};
use localview_windows_uia_provider::{
    WindowsUiaCoordinateSpace, WindowsUiaGeometryReceipt, WindowsUiaGeometryRequest,
};
use serde_json::{Value, json};

use super::verified_input_seed::{
    EdgeSeedProcess, attach_and_snapshot, spawn_worker, truth_bool, truth_u64,
};

const W10_CASE_ID: &str = "W10-mixed-dpi-window-movement";
const W10_CANONICAL_OUTCOME: &str = "live-physical-geometry-refreshed-after-cross-dpi-move";

fn truth_i32(value: &Value, field: &str) -> i32 {
    value
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or_else(|| panic!("W10 geometry oracle field {field} must be i32: {value}"))
}

fn rect_matches_oracle(receipt: &WindowsUiaGeometryReceipt, truth: &Value) -> bool {
    receipt.bounding_rect.left == truth_i32(truth, "window_left")
        && receipt.bounding_rect.top == truth_i32(truth, "window_top")
        && receipt.bounding_rect.right == truth_i32(truth, "window_right")
        && receipt.bounding_rect.bottom == truth_i32(truth, "window_bottom")
}

fn scaled_coordinate(value: i32, dpi: u64) -> Option<i32> {
    let scaled = i64::from(value).checked_mul(i64::try_from(dpi).ok()?)?;
    i32::try_from(scaled / 96).ok()
}

fn looks_double_scaled(receipt: &WindowsUiaGeometryReceipt, truth: &Value) -> bool {
    let dpi = truth_u64(truth, "window_dpi");
    if dpi == 96 || rect_matches_oracle(receipt, truth) {
        return false;
    }

    let expected = [
        truth_i32(truth, "window_left"),
        truth_i32(truth, "window_top"),
        truth_i32(truth, "window_right"),
        truth_i32(truth, "window_bottom"),
    ];
    let observed = [
        receipt.bounding_rect.left,
        receipt.bounding_rect.top,
        receipt.bounding_rect.right,
        receipt.bounding_rect.bottom,
    ];

    expected
        .into_iter()
        .zip(observed)
        .all(|(expected, observed)| scaled_coordinate(expected, dpi) == Some(observed))
}

fn rect_text(receipt: &WindowsUiaGeometryReceipt) -> String {
    format!(
        "{},{},{},{}",
        receipt.bounding_rect.left,
        receipt.bounding_rect.top,
        receipt.bounding_rect.right,
        receipt.bounding_rect.bottom
    )
}

fn oracle_rect_text(truth: &Value) -> String {
    format!(
        "{},{},{},{}",
        truth_i32(truth, "window_left"),
        truth_i32(truth, "window_top"),
        truth_i32(truth, "window_right"),
        truth_i32(truth, "window_bottom")
    )
}

pub async fn run_w10(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
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

    let before_monitor = truth_u64(&before_truth, "monitor_handle");
    let before_dpi = truth_u64(&before_truth, "window_dpi");
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
    let first_rect_matches_oracle = rect_matches_oracle(&before, &before_truth);
    let before_coordinate_space_explicit =
        before.coordinate_space == WindowsUiaCoordinateSpace::PhysicalScreenPixels;
    let before_dpi_matches_oracle = u64::from(before.target_window_dpi) == before_dpi;

    let after_truth = seed.command(json!({ "command": "move_to_alternate_dpi_monitor" }));
    assert_eq!(
        after_truth.get("ok").and_then(Value::as_bool),
        Some(true),
        "W10 seed must perform a real HWND move onto a display with different effective DPI: {after_truth}"
    );
    assert_eq!(truth_u64(&after_truth, "window_handle"), target_window);
    let after_monitor = truth_u64(&after_truth, "monitor_handle");
    let after_dpi = truth_u64(&after_truth, "window_dpi");
    assert_ne!(after_monitor, before_monitor, "W10 must cross a real monitor boundary");
    assert_ne!(after_dpi, before_dpi, "W10 must cross a real effective-DPI boundary");

    let after = worker
        .observe_geometry(&attachment, request)
        .expect("W10 retained live element must refresh geometry after cross-DPI movement");
    let second_rect_matches_oracle = rect_matches_oracle(&after, &after_truth);
    let after_coordinate_space_explicit =
        after.coordinate_space == WindowsUiaCoordinateSpace::PhysicalScreenPixels;
    let after_dpi_matches_oracle = u64::from(after.target_window_dpi) == after_dpi;
    let stale_rect_replayed = after.bounding_rect == before.bounding_rect;
    let stale_dpi_replayed = after.target_window_dpi == before.target_window_dpi;
    let double_scaling_observed = looks_double_scaled(&before, &before_truth)
        || looks_double_scaled(&after, &after_truth);

    let distinct_effective_dpi_observed = before_dpi != after_dpi
        && before_monitor != after_monitor
        && truth_bool(&before_truth, "mixed_dpi_capable")
        && truth_bool(&after_truth, "mixed_dpi_capable");
    let coordinate_space_explicit =
        before_coordinate_space_explicit && after_coordinate_space_explicit;

    let ground_truth = json!({
        "window_handle": target_window,
        "before": {
            "monitor_handle": before_monitor,
            "window_dpi": before_dpi,
            "physical_rect": oracle_rect_text(&before_truth)
        },
        "after": {
            "monitor_handle": after_monitor,
            "window_dpi": after_dpi,
            "physical_rect": oracle_rect_text(&after_truth)
        },
        "mixed_dpi_capable": true,
        "same_hwnd_across_move": true
    });

    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: W10_CASE_ID,
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w10:target-hwnd:{target_window}"),
            format!("windows-uia:w10:before-monitor:{before_monitor}"),
            format!("windows-uia:w10:after-monitor:{after_monitor}"),
            format!("windows-uia:w10:before-oracle-dpi:{before_dpi}"),
            format!("windows-uia:w10:after-oracle-dpi:{after_dpi}"),
            format!("windows-uia:w10:before-shipping-dpi:{}", before.target_window_dpi),
            format!("windows-uia:w10:after-shipping-dpi:{}", after.target_window_dpi),
            format!("windows-uia:w10:before-oracle-rect:{}", oracle_rect_text(&before_truth)),
            format!("windows-uia:w10:after-oracle-rect:{}", oracle_rect_text(&after_truth)),
            format!("windows-uia:w10:before-shipping-rect:{}", rect_text(&before)),
            format!("windows-uia:w10:after-shipping-rect:{}", rect_text(&after)),
            format!("windows-uia:w10:before-dpi-matches-oracle:{before_dpi_matches_oracle}"),
            format!("windows-uia:w10:after-dpi-matches-oracle:{after_dpi_matches_oracle}"),
            format!("windows-uia:w10:stale-rect-replayed:{stale_rect_replayed}"),
            format!("windows-uia:w10:stale-dpi-replayed:{stale_dpi_replayed}"),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: W10_CANONICAL_OUTCOME.into(),
            digest: canonical_digest(&ground_truth).expect("digest W10 independent geometry truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(W10_CANONICAL_OUTCOME.into()),
        case_kind: RealProviderCaseKind::W10MixedDpiGeometry {
            distinct_effective_dpi_observed,
            coordinate_space_explicit,
            first_rect_matches_oracle: first_rect_matches_oracle && before_dpi_matches_oracle,
            second_rect_matches_oracle: second_rect_matches_oracle
                && after_dpi_matches_oracle
                && !stale_rect_replayed
                && !stale_dpi_replayed,
            double_scaling_observed,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W10 mixed-DPI campaign evidence");

    drop(worker);
    seed.shutdown();
    record
}
