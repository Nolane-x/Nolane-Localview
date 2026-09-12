use std::{
    thread,
    time::{Duration, Instant},
};

use localview_live_bridge::DispatchLinearizationReceipt;
use localview_protocol::{DispatchResult, TransportResult};
use localview_windows_observe_runtime::dispatch_result_for_verified_input;
use localview_windows_uia_provider::WindowsInputInsertionClass;

use crate::verified_input_seed::{
    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,
    mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionVerifiedInputFullSmoke {
    pub requested_event_count: u32,
    pub inserted_event_count: u32,
    pub insertion_class: WindowsInputInsertionClass,
    pub reconciliation_required: bool,
    pub dispatch_result: DispatchResult,
    pub effect_count: u64,
    pub target_is_foreground: bool,
}

pub async fn run_production_verified_input_full_smoke(
    seed: &mut EdgeSeedProcess,
    snapshot_cut_ref: &str,
) -> ProductionVerifiedInputFullSmoke {
    let fixture = seed.prepare_input_target();
    let target_window = truth_u64(&fixture, "window_handle");
    assert_ne!(target_window, 0, "W08 production target HWND must be real");
    assert!(
        truth_bool(&fixture, "target_is_foreground"),
        "W08 production dispatch must begin with the exact target foreground"
    );
    assert_eq!(truth_u64(&fixture, "effect_count"), 0);

    let worker = spawn_worker();
    let (attachment, snapshot) =
        attach_and_snapshot(&worker, seed, target_window, snapshot_cut_ref);
    let target = snapshot
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("production UIA snapshot must retain the deterministic W08 input target");
    let authority =
        mint_verified_input_authority(&attachment, snapshot.as_ref(), target.element_ref.clone())
            .await;

    let receipt = worker
        .dispatch_verified_input(&attachment, authority.request)
        .expect("W08 production input must traverse journal-minted exact UIA worker authority");
    let boundary = receipt.boundary();
    let requested_event_count = boundary.requested_event_count;
    let inserted_event_count = boundary.inserted_event_count;
    let insertion_class = boundary.insertion_class;
    let reconciliation_required = boundary.reconciliation_required;
    let dispatch_result = dispatch_result_for_verified_input(boundary);
    let dispatch_attempt_ref = receipt.dispatch_attempt_ref();

    assert_eq!(requested_event_count, 2);
    assert_eq!(inserted_event_count, 2);
    assert_eq!(insertion_class, WindowsInputInsertionClass::FullyInserted);
    assert!(
        reconciliation_required,
        "full platform insertion remains world-unverified until fresh observation"
    );
    assert_eq!(dispatch_result, DispatchResult::DispatchedFull);

    authority
        .journal
        .record_dispatch_linearized(
            authority.permit,
            DispatchLinearizationReceipt {
                receipt_ref: format!("dispatch:v43-real-input:{dispatch_attempt_ref}"),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result,
            },
        )
        .await
        .expect("W08 production provider receipt must consume the exact one-shot journal permit");

    let deadline = Instant::now() + Duration::from_secs(2);
    let after = loop {
        let state = seed.input_state();
        if truth_u64(&state, "effect_count") > 0 {
            break state;
        }
        assert!(
            Instant::now() < deadline,
            "W08 full worker dispatch returned but independent WPF oracle observed no Space effect"
        );
        thread::sleep(Duration::from_millis(25));
    };
    let effect_count = truth_u64(&after, "effect_count");
    let target_is_foreground = truth_bool(&after, "target_is_foreground");
    assert_eq!(
        effect_count, 1,
        "one authority-bound worker dispatch must produce exactly one WPF Space keydown effect"
    );
    assert!(target_is_foreground);

    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);
    drop(worker);

    ProductionVerifiedInputFullSmoke {
        requested_event_count,
        inserted_event_count,
        insertion_class,
        reconciliation_required,
        dispatch_result,
        effect_count,
        target_is_foreground,
    }
}
