#![cfg(windows)]

use std::{collections::BTreeSet, fs, path::Path};

use localview_validation_lab::{
    RealProviderCaseInput, RealProviderCaseKind, RealProviderGroundTruth,
    RealProviderLabRecord, RealProviderObservedOutcome, ResultEvidence,
    adapt_real_provider_case, canonical_digest,
};
use localview_windows_uia_provider::{
    WindowsUiaDispatchContextBlocker, WindowsUiaWorkerError,
    WindowsVerifiedInputBoundaryError,
};
use serde_json::{Value, json};

use crate::verified_input_seed::{
    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, VerifiedInputAuthority,
    attach_and_snapshot, mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,
};

const W07_CASE_ID: &str = "W07-foreground-stolen-before-input";
const W08_CASE_ID: &str = "W08-partial-input-dispatch";
const W09_CASE_ID: &str = "W09-user-held-modifier-interference";

async fn consume_blocked_authority(authority: VerifiedInputAuthority) {
    authority
        .journal
        .abandon_dispatch_execution(authority.permit)
        .await
        .expect("blocked verified-input campaign case must consume volatile dispatch authority");
    drop(authority.journal);
    let _ = fs::remove_file(authority.journal_path);
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
    assert_ne!(target_window, 0);
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
        .expect("campaign W07 must retain exact verified-input target");
    let authority = mint_verified_input_authority(
        &attachment,
        snapshot.as_ref(),
        target.element_ref.clone(),
    )
    .await;

    let stolen = seed.steal_foreground();
    let thief_window = truth_u64(&stolen, "thief_window_handle");
    assert_ne!(thief_window, target_window);
    assert_eq!(truth_u64(&stolen, "foreground_window_handle"), thief_window);

    let VerifiedInputAuthority {
        journal,
        journal_path,
        permit,
        request,
    } = authority;
    let error = worker
        .dispatch_verified_input(&attachment, request)
        .expect_err("campaign W07 foreground theft must fail closed before SendInput");
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
    let input_inserted = truth_u64(&after, "effect_count") != 0;
    assert!(!input_inserted);
    assert!(!truth_bool(&after, "target_is_foreground"));

    journal
        .abandon_dispatch_execution(permit)
        .await
        .expect("W07 blocked campaign attempt must consume only volatile execution authority");
    drop(journal);
    let _ = fs::remove_file(journal_path);
    seed.shutdown();

    let canonical_outcome = "foreground-theft-blocked-before-input";
    let truth = json!({
        "target_window_handle": target_window,
        "thief_window_handle": thief_window,
        "final_foreground_mismatch_detected": true,
        "input_inserted": input_inserted,
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: W07_CASE_ID,
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w07:target-hwnd:{target_window}"),
            format!("windows-uia:w07:thief-hwnd:{thief_window}"),
            "windows-uia:w07:typed-blocker:foreground-window-mismatch".into(),
            "wpf-oracle:w07:effect-count:0".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: canonical_outcome.into(),
            digest: canonical_digest(&truth).expect("digest W07 independent oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(canonical_outcome.into()),
        case_kind: RealProviderCaseKind::W07ForegroundStolen {
            final_foreground_mismatch_detected: true,
            input_inserted,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt exact W07 real-provider campaign evidence");
    assert_eq!(record.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
    record
}

fn read_artifact(path: &Path) -> Value {
    let bytes = fs::read(path)
        .unwrap_or_else(|error| panic!("required W08 evidence artifact {} missing: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("parse W08 evidence artifact {}: {error}", path.display()))
}

pub fn run_w08(
    artifact_dir: &Path,
    edge_seed_digest: &str,
    environment_digest: &str,
    platform_profile: &str,
    comparison_profile: &str,
    logical_sequence: u64,
) -> RealProviderLabRecord {
    let wrapper_path = artifact_dir.join("W08-DETERMINISTIC-PARTIAL-WRAPPER.json");
    let smoke_path = artifact_dir.join("W08-PRODUCTION-FULL-SMOKE.json");
    let wrapper = read_artifact(&wrapper_path);
    let smoke = read_artifact(&smoke_path);

    assert_eq!(wrapper.get("case_id").and_then(Value::as_str), Some(W08_CASE_ID));
    assert_eq!(wrapper.get("requested_event_count").and_then(Value::as_u64), Some(4));
    assert_eq!(wrapper.get("inserted_event_count").and_then(Value::as_u64), Some(2));
    assert_eq!(wrapper.get("reconciliation_required").and_then(Value::as_bool), Some(true));
    assert_eq!(wrapper.get("blind_retry_authorized").and_then(Value::as_bool), Some(false));
    assert_eq!(wrapper.get("natural_windows_partial_observed").and_then(Value::as_bool), Some(false));

    assert_eq!(smoke.get("case_id").and_then(Value::as_str), Some(W08_CASE_ID));
    assert_eq!(smoke.get("requested_event_count").and_then(Value::as_u64), Some(2));
    assert_eq!(smoke.get("inserted_event_count").and_then(Value::as_u64), Some(2));
    assert_eq!(smoke.get("independent_oracle_effect_count").and_then(Value::as_u64), Some(1));
    assert_eq!(smoke.get("natural_windows_partial_observed").and_then(Value::as_bool), Some(false));

    let wrapper_digest = canonical_digest(&wrapper).expect("digest deterministic W08 wrapper evidence");
    let smoke_digest = canonical_digest(&smoke).expect("digest production W08 full-smoke evidence");
    let canonical_outcome = "partial-dispatch-preserved-unknown-with-no-retry";
    let truth = json!({
        "requested_event_count": 4,
        "inserted_event_count": 2,
        "unknown_outcome_preserved": true,
        "blind_retry_authorized": false,
        "deterministic_wrapper_digest": wrapper_digest.0,
        "production_full_smoke_digest": smoke_digest.0,
        "natural_windows_partial_observed": false,
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: W08_CASE_ID,
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("w08:deterministic-wrapper:{}", wrapper_digest.0),
            format!("w08:production-full-smoke:{}", smoke_digest.0),
            "w08:claim-boundary:no-natural-windows-partial-observed".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: canonical_outcome.into(),
            digest: canonical_digest(&truth).expect("digest combined W08 evidence truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(canonical_outcome.into()),
        case_kind: RealProviderCaseKind::W08PartialInputDispatch {
            requested_event_count: 4,
            inserted_event_count: 2,
            unknown_outcome_preserved: true,
            blind_retry_authorized: false,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt bounded W08 campaign evidence");
    assert_eq!(record.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
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
        .expect("campaign W09 must retain exact verified-input target");
    let authority = mint_verified_input_authority(
        &attachment,
        snapshot.as_ref(),
        target.element_ref.clone(),
    )
    .await;

    let held = seed.hold_shift();
    let conflicting_modifier_observed = truth_bool(&held, "shift_down");
    assert!(conflicting_modifier_observed);

    let VerifiedInputAuthority {
        journal,
        journal_path,
        permit,
        request,
    } = authority;
    let error = worker
        .dispatch_verified_input(&attachment, request)
        .expect_err("campaign W09 held Shift must fail closed before SendInput");
    let input_state_conflict_blocked = error
        == WindowsUiaWorkerError::VerifiedInputBoundary(
            WindowsVerifiedInputBoundaryError::InputStateConflict,
        );
    assert!(input_state_conflict_blocked, "W09 must preserve typed InputStateConflict: {error:?}");

    let after = seed.input_state();
    assert!(truth_bool(&after, "shift_down"), "LocalView must not normalize/release held Shift");
    let input_inserted = truth_u64(&after, "effect_count") != 0;
    assert!(!input_inserted);
    seed.release_shift();

    journal
        .abandon_dispatch_execution(permit)
        .await
        .expect("W09 blocked campaign attempt must consume only volatile execution authority");
    drop(journal);
    let _ = fs::remove_file(journal_path);
    seed.shutdown();

    let canonical_outcome = "held-modifier-blocked-without-normalization-or-input";
    let truth = json!({
        "conflicting_modifier_observed": conflicting_modifier_observed,
        "input_state_conflict_blocked": input_state_conflict_blocked,
        "input_inserted": input_inserted,
        "localview_released_modifier": false,
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: W09_CASE_ID,
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            "windows-uia:w09:typed-blocker:input-state-conflict".into(),
            "wpf-oracle:w09:shift-remained-down-after-block".into(),
            "wpf-oracle:w09:effect-count:0".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: canonical_outcome.into(),
            digest: canonical_digest(&truth).expect("digest W09 independent oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(canonical_outcome.into()),
        case_kind: RealProviderCaseKind::W09ModifierInterference {
            conflicting_modifier_observed,
            input_state_conflict_blocked,
            input_inserted,
        },
        comparison_profile_revision: comparison_profile,
        logical_sequence,
    })
    .expect("adapt exact W09 real-provider campaign evidence");
    assert_eq!(record.result_evidence, Some(ResultEvidence::RealProviderIntegrationPass));
    record
}
