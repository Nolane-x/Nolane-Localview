use std::collections::BTreeSet;

use localview_validation_lab::{
    RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth, RealProviderLabRecord,
    RealProviderObservedOutcome, adapt_real_provider_case, canonical_digest,
};
use localview_windows_uia_provider::{
    WindowsUiaDispatchContextBlocker, WindowsUiaWorkerError,
};
use serde_json::json;

use super::verified_input_seed::{
    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, abandon_authority, attach_and_snapshot,
    mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
};

pub async fn run_w11(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed = EdgeSeedProcess::spawn();
    let fixture = seed.prepare_input_target();
    let target_window = truth_u64(&fixture, "window_handle");
    assert_ne!(target_window, 0);
    assert!(truth_bool(&fixture, "target_is_foreground"));
    assert_eq!(truth_u64(&fixture, "effect_count"), 0);

    let worker = spawn_worker();
    let (attachment, snapshot) = attach_and_snapshot(
        &worker,
        &seed,
        target_window,
        "cut:v43:campaign:w11:before-modal",
    );
    let target = snapshot
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W11 snapshot must retain deterministic input target");
    let authority =
        mint_verified_input_authority(&attachment, snapshot.as_ref(), target.element_ref.clone())
            .await;

    let modal = seed.open_modal_blocker();
    let modal_window = truth_u64(&modal, "modal_window_handle");
    let modal_owner = truth_u64(&modal, "modal_owner_window_handle");
    assert_ne!(modal_window, 0);
    assert_ne!(modal_window, target_window);
    assert_eq!(modal_owner, target_window);
    assert!(truth_bool(&modal, "modal_is_open"));
    assert_eq!(truth_u64(&modal, "effect_count"), 0);

    let error = worker
        .dispatch_verified_input(&attachment, authority.request)
        .expect_err("W11 owned modal must block before SendInput");
    assert_eq!(
        error,
        WindowsUiaWorkerError::DispatchContextBlocked(
            WindowsUiaDispatchContextBlocker::ModalBlockerPresent {
                window_handle: modal_window,
            }
        )
    );
    let after = seed.input_state();
    assert_eq!(truth_u64(&after, "effect_count"), 0);
    assert!(truth_bool(&after, "modal_is_open"));

    seed.close_modal_blocker();
    authority
        .journal
        .abandon_dispatch_execution(authority.permit)
        .await
        .expect("W11 block must consume only volatile execution authority");
    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);

    let truth = json!({
        "target_window_handle": target_window,
        "modal_window_handle": modal_window,
        "modal_owner_window_handle": modal_owner,
        "effect_count": truth_u64(&after, "effect_count"),
        "final_blocker": "modal_blocker_present"
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W11-modal-before-dispatch",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w11:target-hwnd:{target_window}"),
            format!("windows-uia:w11:modal-hwnd:{modal_window}"),
            format!("windows-uia:w11:modal-owner-hwnd:{modal_owner}"),
            "windows-uia:w11:typed-blocker:modal-blocker-present".into(),
            "windows-uia:w11:oracle-effect-count:0".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "owned-modal-blocked-before-input".into(),
            digest: canonical_digest(&truth).expect("digest W11 campaign truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "owned-modal-blocked-before-input".into(),
        ),
        case_kind: RealProviderCaseKind::W11ModalBeforeDispatch {
            modal_blocker_observed: true,
            input_inserted: false,
            target_effect_observed: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W11 campaign evidence");

    drop(worker);
    seed.shutdown();
    record
}

pub async fn run_w12(
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let mut seed_a = EdgeSeedProcess::spawn();
    let fixture_a = seed_a.prepare_input_target();
    let a_window = truth_u64(&fixture_a, "window_handle");
    let a_process = seed_a.process_id();
    assert_ne!(a_window, 0);
    assert_eq!(truth_u64(&fixture_a, "effect_count"), 0);

    let worker = spawn_worker();
    let (attachment_a, snapshot_a) = attach_and_snapshot(
        &worker,
        &seed_a,
        a_window,
        "cut:v43:campaign:w12:process-a",
    );
    let target_a = snapshot_a
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W12 snapshot must retain process-A target");
    let authority_a = mint_verified_input_authority(
        &attachment_a,
        snapshot_a.as_ref(),
        target_a.element_ref.clone(),
    )
    .await;
    let a_target = attachment_a.target_incarnation_ref().clone();
    let a_fingerprint = attachment_a.fingerprint().clone();

    seed_a.kill_and_wait();

    let mut seed_b = EdgeSeedProcess::spawn();
    let fixture_b = seed_b.prepare_input_target();
    let b_window = truth_u64(&fixture_b, "window_handle");
    let b_process = seed_b.process_id();
    assert_ne!(b_window, 0);
    assert_eq!(truth_u64(&fixture_b, "effect_count"), 0);

    let stale_error = worker
        .dispatch_verified_input(&attachment_a, authority_a.request)
        .expect_err("W12 process-A authority must fail after A terminates");
    assert_eq!(stale_error, WindowsUiaWorkerError::TargetReincarnated);
    let after_stale = seed_b.input_state();
    assert_eq!(truth_u64(&after_stale, "effect_count"), 0);

    let (attachment_b, snapshot_b) = attach_and_snapshot(
        &worker,
        &seed_b,
        b_window,
        "cut:v43:campaign:w12:process-b",
    );
    assert_ne!(attachment_b.fingerprint(), &a_fingerprint);
    assert_ne!(attachment_b.target_incarnation_ref(), &a_target);
    let b_target = attachment_b.target_incarnation_ref().clone();
    let target_b = snapshot_b
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W12 snapshot must reacquire process-B target");
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
        .expect("W12 stale A attempt must consume only volatile authority");
    drop(authority_a.journal);
    let _ = std::fs::remove_file(authority_a.journal_path);

    let truth = json!({
        "process_a": a_process,
        "window_a": a_window,
        "target_incarnation_a": format!("{a_target:?}"),
        "process_b": b_process,
        "window_b": b_window,
        "target_incarnation_b": format!("{b_target:?}"),
        "replacement_effect_count": truth_u64(&after_stale, "effect_count"),
        "final_blocker": "target_reincarnated"
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W12-target-restart-after-authorization",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w12:a-pid:{a_process}"),
            format!("windows-uia:w12:a-hwnd:{a_window}"),
            format!("windows-uia:w12:a-target:{a_target:?}"),
            format!("windows-uia:w12:b-pid:{b_process}"),
            format!("windows-uia:w12:b-hwnd:{b_window}"),
            format!("windows-uia:w12:b-target:{b_target:?}"),
            "windows-uia:w12:typed-blocker:target-reincarnated".into(),
            "windows-uia:w12:replacement-effect-count:0".into(),
            "windows-uia:w12:fresh-reacquire-required:true".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "stale-pre-restart-authority-rejected-before-replacement-effect".into(),
            digest: canonical_digest(&truth).expect("digest W12 campaign truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "stale-pre-restart-authority-rejected-before-replacement-effect".into(),
        ),
        case_kind: RealProviderCaseKind::W12TargetRestartAfterAuthorization {
            original_target_gone: true,
            replacement_target_present: true,
            stale_authority_rejected: true,
            replacement_effect_observed: false,
            fresh_reacquire_required: true,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt W12 campaign evidence");

    drop(worker);
    seed_b.shutdown();
    record
}
