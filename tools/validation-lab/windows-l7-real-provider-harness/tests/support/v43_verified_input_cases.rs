use std::collections::BTreeSet;

#[path = "v43_verified_input_full_smoke.rs"]
mod full_smoke;

use localview_protocol::DispatchResult;
use localview_validation_lab::{
    RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth, RealProviderLabRecord,
    RealProviderObservedOutcome, adapt_real_provider_case, canonical_digest,
};
use localview_windows_observe_runtime::dispatch_result_for_verified_input;
use localview_windows_uia_provider::{
    WindowsInputInsertRawResult, WindowsInputInsertionClass, WindowsKeyTransition,
    WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextBlocker,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements,
    WindowsUiaWorkerError, WindowsVerifiedInputBoundaryError, WindowsVerifiedInputEnvironment,
    WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch, execute_windows_verified_input_boundary,
};
use serde_json::json;

use super::verified_input_seed::{
    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,
    mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
};

const W08_SYNTHETIC_HWND: u64 = 0x4308;
const W08_SYNTHETIC_PID: u32 = 4308;

#[derive(Debug, Default)]
struct DeterministicPartialInserter {
    insert_calls: u32,
}

impl WindowsVerifiedInputEnvironment for DeterministicPartialInserter {
    fn observe_dispatch_context(
        &mut self,
    ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError> {
        Ok(WindowsUiaDispatchContextObservation {
            target_window_handle: W08_SYNTHETIC_HWND,
            target_process_id: W08_SYNTHETIC_PID,
            foreground_window_handle: Some(W08_SYNTHETIC_HWND),
            foreground_process_id: Some(W08_SYNTHETIC_PID),
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

    fn insert_events(&mut self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult {
        self.insert_calls += 1;
        assert_eq!(
            events.len(),
            4,
            "W08 campaign wrapper must receive exact four-event batch"
        );
        WindowsInputInsertRawResult {
            requested_event_count: 4,
            inserted_event_count: 2,
            raw_error_code: None,
        }
    }
}

fn context_requirements() -> WindowsUiaDispatchContextRequirements {
    WindowsUiaDispatchContextRequirements {
        require_foreground_target: true,
        require_exact_element_focus: false,
        require_no_modal_blocker: true,
    }
}

fn four_event_wrapper_batch() -> WindowsVerifiedKeyboardBatch {
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
    .expect("construct deterministic W08 four-event wrapper batch")
}

pub async fn run_w07(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed = EdgeSeedProcess::spawn();
    let fixture = seed.prepare_input_target();
    let target_window = truth_u64(&fixture, "window_handle");
    assert_ne!(target_window, 0, "W07 campaign target HWND must be real");
    assert!(truth_bool(&fixture, "target_is_foreground"));
    assert_eq!(truth_u64(&fixture, "effect_count"), 0);

    let worker = spawn_worker();
    let (attachment, snapshot) = attach_and_snapshot(
        &worker,
        &seed,
        target_window,
        "cut:v43:campaign:w07:before-foreground-theft",
    );
    let target = snapshot
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W07 snapshot must retain deterministic input target");
    let authority =
        mint_verified_input_authority(&attachment, snapshot.as_ref(), target.element_ref.clone())
            .await;

    let stolen = seed.steal_foreground();
    let thief_window = truth_u64(&stolen, "thief_window_handle");
    assert_ne!(thief_window, target_window);
    assert_eq!(truth_u64(&stolen, "foreground_window_handle"), thief_window);

    let error = worker
        .dispatch_verified_input(&attachment, authority.request)
        .expect_err("campaign W07 must block foreground theft before SendInput");
    assert_eq!(
        error,
        WindowsUiaWorkerError::DispatchContextBlocked(
            WindowsUiaDispatchContextBlocker::ForegroundWindowMismatch {
                expected: target_window,
                actual: thief_window,
            }
        )
    );
    let after = seed.input_state();
    assert_eq!(truth_u64(&after, "effect_count"), 0);
    assert!(!truth_bool(&after, "target_is_foreground"));

    authority
        .journal
        .abandon_dispatch_execution(authority.permit)
        .await
        .expect("campaign W07 block must consume only volatile dispatch authority");
    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);

    let ground_truth = json!({
        "target_window_handle": target_window,
        "foreground_thief_window_handle": thief_window,
        "final_foreground_window_handle": truth_u64(&after, "foreground_window_handle"),
        "target_is_foreground": truth_bool(&after, "target_is_foreground"),
        "effect_count": truth_u64(&after, "effect_count"),
        "final_blocker": "foreground_window_mismatch"
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W07-foreground-stolen-before-input",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w07:target-hwnd:{target_window}"),
            format!("windows-uia:w07:thief-hwnd:{thief_window}"),
            "windows-uia:w07:typed-blocker:foreground-window-mismatch".into(),
            "windows-uia:w07:oracle-effect-count:0".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "final-foreground-mismatch-blocked-before-input".into(),
            digest: canonical_digest(&ground_truth).expect("digest W07 campaign oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "final-foreground-mismatch-blocked-before-input".into(),
        ),
        case_kind: RealProviderCaseKind::W07ForegroundStolen {
            final_foreground_mismatch_detected: true,
            input_inserted: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W07 campaign evidence");

    drop(worker);
    seed.shutdown();
    record
}

pub async fn run_w08(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut wrapper = DeterministicPartialInserter::default();
    let partial = execute_windows_verified_input_boundary(
        context_requirements(),
        &four_event_wrapper_batch(),
        &mut wrapper,
    )
    .expect("campaign W08 deterministic wrapper must traverse production classifier");
    assert_eq!(partial.requested_event_count, 4);
    assert_eq!(partial.inserted_event_count, 2);
    assert_eq!(
        partial.insertion_class,
        WindowsInputInsertionClass::PartialDispatchUnknownOutcome
    );
    assert!(partial.reconciliation_required);
    assert_eq!(
        dispatch_result_for_verified_input(&partial),
        DispatchResult::DispatchedPartial
    );
    assert_eq!(
        wrapper.insert_calls, 1,
        "W08 partial outcome must not blind-retry"
    );

    let mut seed = EdgeSeedProcess::spawn();
    let full = full_smoke::run_production_verified_input_full_smoke(
        &mut seed,
        "cut:v43:campaign:w08:production-full",
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

    let ground_truth = json!({
        "partial_wrapper": {
            "requested_event_count": partial.requested_event_count,
            "inserted_event_count": partial.inserted_event_count,
            "classification": "partial_dispatch_unknown_outcome",
            "reconciliation_required": partial.reconciliation_required,
            "blind_retry_authorized": false,
            "insert_call_count": wrapper.insert_calls,
            "natural_windows_partial_observed": false
        },
        "production_windows_full_smoke": {
            "requested_event_count": full.requested_event_count,
            "inserted_event_count": full.inserted_event_count,
            "classification": "fully_inserted",
            "reconciliation_required": full.reconciliation_required,
            "dispatch_result": "dispatched_full",
            "authority_path": "journal-minted-request-to-exact-uia-worker",
            "oracle_effect_count": full.effect_count
        },
        "claim_boundary": "partial-count evidence is deterministic wrapper/property evidence; hosted Windows production evidence proves ordinary full insertion only through journal-minted exact UIA worker authority"
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W08-partial-input-dispatch",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            "windows-uia:w08:partial-source:deterministic-wrapper".into(),
            "windows-uia:w08:natural-windows-partial-observed:false".into(),
            "windows-uia:w08:wrapper-counts:requested=4,inserted=2".into(),
            "windows-uia:w08:wrapper-insert-calls:1".into(),
            "windows-uia:w08:runtime-mapping:dispatched-partial".into(),
            "windows-uia:w08:production-worker-full:requested=2,inserted=2".into(),
            "windows-uia:w08:production-authority-path:journal-minted-exact-uia-worker".into(),
            "windows-uia:w08:production-oracle-effect-count:1".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "partial-unknown-preserved-with-production-backend-wiring".into(),
            digest: canonical_digest(&ground_truth).expect("digest W08 mixed-provenance truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "partial-unknown-preserved-with-production-backend-wiring".into(),
        ),
        case_kind: RealProviderCaseKind::W08PartialInputDispatch {
            requested_event_count: partial.requested_event_count,
            inserted_event_count: partial.inserted_event_count,
            unknown_outcome_preserved: true,
            blind_retry_authorized: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W08 campaign evidence with explicit wrapper provenance");

    seed.shutdown();
    record
}

pub async fn run_w09(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed = EdgeSeedProcess::spawn();
    let fixture = seed.prepare_input_target();
    let target_window = truth_u64(&fixture, "window_handle");
    assert!(truth_bool(&fixture, "target_is_foreground"));
    assert_eq!(truth_u64(&fixture, "effect_count"), 0);

    let worker = spawn_worker();
    let (attachment, snapshot) = attach_and_snapshot(
        &worker,
        &seed,
        target_window,
        "cut:v43:campaign:w09:before-modifier-conflict",
    );
    let target = snapshot
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W09 snapshot must retain deterministic input target");
    let authority =
        mint_verified_input_authority(&attachment, snapshot.as_ref(), target.element_ref.clone())
            .await;

    let held = seed.hold_shift();
    assert!(truth_bool(&held, "shift_down"));
    assert_eq!(truth_u64(&held, "effect_count"), 0);
    let error = worker
        .dispatch_verified_input(&attachment, authority.request)
        .expect_err("campaign W09 held Shift must block before SendInput");
    assert_eq!(
        error,
        WindowsUiaWorkerError::VerifiedInputBoundary(
            WindowsVerifiedInputBoundaryError::InputStateConflict
        )
    );
    let after = seed.input_state();
    assert!(truth_bool(&after, "shift_down"));
    assert_eq!(truth_u64(&after, "effect_count"), 0);

    seed.release_shift();
    authority
        .journal
        .abandon_dispatch_execution(authority.permit)
        .await
        .expect("campaign W09 block must consume only volatile dispatch authority");
    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);

    let ground_truth = json!({
        "conflicting_modifier": "shift",
        "shift_observed_down_before_dispatch": true,
        "shift_still_down_after_localview_block": truth_bool(&after, "shift_down"),
        "effect_count": truth_u64(&after, "effect_count"),
        "final_blocker": "input_state_conflict",
        "normalization_attempted": false
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W09-user-held-modifier-interference",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            "windows-uia:w09:modifier:shift-down".into(),
            "windows-uia:w09:typed-blocker:input-state-conflict".into(),
            "windows-uia:w09:modifier-remained-down-after-block:true".into(),
            "windows-uia:w09:oracle-effect-count:0".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "held-modifier-blocked-without-normalization-or-input".into(),
            digest: canonical_digest(&ground_truth).expect("digest W09 campaign oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "held-modifier-blocked-without-normalization-or-input".into(),
        ),
        case_kind: RealProviderCaseKind::W09ModifierInterference {
            conflicting_modifier_observed: true,
            input_state_conflict_blocked: true,
            input_inserted: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W09 campaign evidence");

    drop(worker);
    seed.shutdown();
    record
}
