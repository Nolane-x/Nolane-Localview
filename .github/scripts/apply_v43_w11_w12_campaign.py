from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one anchor, found {count}")
    return text.replace(old, new, 1)


root = Path(__file__).resolve().parents[2]

# 1. Connect the already-proven W11/W12 real-provider paths to typed Lab records.
cases_path = root / "tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_cases.rs"
cases = cases_path.read_text(encoding="utf-8")
cases = replace_once(
    cases,
    "use super::verified_input_seed::{\n    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, attach_and_snapshot,\n    mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,\n};",
    "use super::verified_input_seed::{\n    EdgeSeedProcess, INPUT_TARGET_AUTOMATION_ID, abandon_authority, attach_and_snapshot,\n    mint_verified_input_authority, spawn_worker, truth_bool, truth_u64,\n};",
    "verified-input support import",
)
if "pub async fn run_w11(" in cases or "pub async fn run_w12(" in cases:
    raise SystemExit("W11/W12 campaign helpers already exist; refusing duplicate generation")

cases += r'''

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
    assert_ne!(target_window, 0, "W11 campaign target HWND must be real");
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

    let error = worker
        .dispatch_verified_input(&attachment, authority.request)
        .expect_err("campaign W11 owned modal must block before SendInput");
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
        .expect("campaign W11 block must consume only volatile dispatch authority");
    drop(authority.journal);
    let _ = std::fs::remove_file(authority.journal_path);

    let ground_truth = json!({
        "target_window_handle": target_window,
        "modal_window_handle": modal_window,
        "modal_owner_window_handle": modal_owner,
        "modal_blocker_observed": true,
        "input_inserted": false,
        "target_effect_observed": false,
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
            digest: canonical_digest(&ground_truth).expect("digest W11 campaign oracle truth"),
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
    assert_ne!(a_window, 0, "W12 campaign process-A HWND must be real");

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
        .expect("campaign W12 snapshot must retain process-A input target");
    let authority_a = mint_verified_input_authority(
        &attachment_a,
        snapshot_a.as_ref(),
        target_a.element_ref.clone(),
    )
    .await;
    let a_target_incarnation = attachment_a.target_incarnation_ref().clone();
    let a_fingerprint = attachment_a.fingerprint().clone();
    let a_target_incarnation_text = format!("{:?}", a_target_incarnation);

    seed_a.kill_and_wait();

    let mut seed_b = EdgeSeedProcess::spawn();
    let fixture_b = seed_b.prepare_input_target();
    let b_window = truth_u64(&fixture_b, "window_handle");
    let b_process = seed_b.process_id();
    assert_ne!(b_window, 0, "W12 campaign process-B HWND must be real");
    assert_eq!(truth_u64(&fixture_b, "effect_count"), 0);

    let stale_error = worker
        .dispatch_verified_input(&attachment_a, authority_a.request)
        .expect_err("campaign W12 process-A authority must be stale after A exits");
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
    assert_ne!(attachment_b.target_incarnation_ref(), &a_target_incarnation);
    let b_target_incarnation_text = format!("{:?}", attachment_b.target_incarnation_ref());
    let target_b = snapshot_b
        .nodes()
        .iter()
        .find(|node| node.automation_id.as_deref() == Some(INPUT_TARGET_AUTOMATION_ID))
        .expect("campaign W12 must reacquire same logical target in process B");
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
        .expect("campaign W12 stale attempt must consume only volatile process-A authority");
    drop(authority_a.journal);
    let _ = std::fs::remove_file(authority_a.journal_path);

    let ground_truth = json!({
        "process_a": { "pid": a_process, "window_handle": a_window, "target_incarnation": a_target_incarnation_text },
        "process_b": { "pid": b_process, "window_handle": b_window, "target_incarnation": b_target_incarnation_text },
        "original_target_gone": true,
        "replacement_target_present": true,
        "stale_authority_rejected": true,
        "replacement_effect_observed": false,
        "replacement_effect_count": truth_u64(&after_stale, "effect_count"),
        "fresh_reacquire_required": true,
        "typed_rejection": "target_reincarnated"
    });
    let record = adapt_real_provider_case(RealProviderCaseInput {
        case_id: "W12-target-restart-after-authorization",
        seed_app_digest: edge_seed_digest,
        platform_profile_revision: platform_profile,
        environment_artifact_digest: environment_digest,
        provider_evidence_refs: BTreeSet::from([
            format!("windows-uia:w12:process-a-pid:{a_process}"),
            format!("windows-uia:w12:process-a-hwnd:{a_window}"),
            format!("windows-uia:w12:process-b-pid:{b_process}"),
            format!("windows-uia:w12:process-b-hwnd:{b_window}"),
            format!("windows-uia:w12:process-a-target:{a_target_incarnation_text}"),
            format!("windows-uia:w12:process-b-target:{b_target_incarnation_text}"),
            "windows-uia:w12:typed-rejection:target-reincarnated".into(),
            "windows-uia:w12:replacement-effect-count:0".into(),
            "windows-uia:w12:fresh-reacquire-required:true".into(),
        ]),
        ground_truth: RealProviderGroundTruth {
            canonical_outcome: "stale-process-authority-rejected-before-replacement-effect".into(),
            digest: canonical_digest(&ground_truth).expect("digest W12 campaign oracle truth"),
        },
        observed_outcome: RealProviderObservedOutcome::Asserted(
            "stale-process-authority-rejected-before-replacement-effect".into(),
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
'''
cases_path.write_text(cases, encoding="utf-8")

# 2. Expand prospective campaign from exact 9-case set to exact 11-case set (W10 stays unmeasured).
campaign_path = root / "tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_campaign.rs"
campaign = campaign_path.read_text(encoding="utf-8")
campaign = replace_once(
    campaign,
    'const REQUIRED_CASES: [&str; 9] = [\n        "W01-missing-uia-property-event",\n        "W02-recreated-uia-element",\n        "W03-virtualized-item-realization",\n        "W04-unsupported-invoke-pattern",\n        "W05-windows-uia-provider-hang",\n        "W06-windows-uia-provider-reacquire",\n        "W07-foreground-stolen-before-input",\n        "W08-partial-input-dispatch",\n        "W09-user-held-modifier-interference",\n    ];',
    'const REQUIRED_CASES: [&str; 11] = [\n        "W01-missing-uia-property-event",\n        "W02-recreated-uia-element",\n        "W03-virtualized-item-realization",\n        "W04-unsupported-invoke-pattern",\n        "W05-windows-uia-provider-hang",\n        "W06-windows-uia-provider-reacquire",\n        "W07-foreground-stolen-before-input",\n        "W08-partial-input-dispatch",\n        "W09-user-held-modifier-interference",\n        "W11-modal-before-dispatch",\n        "W12-target-restart-after-authorization",\n    ];',
    "required case set",
)
campaign = replace_once(
    campaign,
    "async fn prospective_l7_campaign_binds_w01_through_w09_to_exact_candidate()",
    "async fn prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate()",
    "campaign test name",
)
campaign = campaign.replace("nine-seed", "eleven-seed").replace("W01-W09", "W01-W09/W11/W12")
campaign = replace_once(
    campaign,
    'lab_revision: "lab-v43-windows-l7-r3".into(),\n                seed_corpus_revision: "windows-provider-seeds-w01-w09-r3".into(),',
    'lab_revision: "lab-v43-windows-l7-r4".into(),\n                seed_corpus_revision: "windows-provider-seeds-w01-w09-w11-w12-r4".into(),',
    "campaign revisions",
)
w09_identity = '''                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[8].into(),
                    prediction_revision: "w09-modifier-interference-r1".into(),
                    oracle_revision: "independent-wpf-input-oracle-r1".into(),
                },'''
w11_w12_identities = w09_identity + '''
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[9].into(),
                    prediction_revision: "w11-modal-before-dispatch-r1".into(),
                    oracle_revision: "independent-wpf-modal-oracle-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[10].into(),
                    prediction_revision: "w12-target-restart-after-authorization-r1".into(),
                    oracle_revision: "independent-wpf-process-lifecycle-oracle-r1".into(),
                },'''
campaign = replace_once(campaign, w09_identity, w11_w12_identities, "W11/W12 seed identities")
campaign = replace_once(
    campaign,
    '                "user-held modifier != LocalView normalization authority".into(),',
    '                "user-held modifier != LocalView normalization authority".into(),\n                "clean preflight target != modal-free final dispatch boundary".into(),\n                "logical target similarity after restart != reusable process-A authority".into(),',
    "W11/W12 expected distinctions",
)
w09_record = '''            verified_input_cases::run_w09(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                109,
            )
            .await,'''
w11_w12_records = w09_record + '''
            verified_input_cases::run_w11(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                110,
            )
            .await,
            verified_input_cases::run_w12(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                111,
            )
            .await,'''
campaign = replace_once(campaign, w09_record, w11_w12_records, "W11/W12 campaign records")
campaign = campaign.replace(
    '"the prospective campaign must execute exactly W01 through W09"',
    '"the prospective campaign must execute exactly W01-W09 plus W11 and W12; W10 remains unmeasured"',
)
campaign = campaign.replace(
    '"every required W01-W09 case must be complete before campaign finalization"',
    '"every required W01-W09/W11/W12 case must be complete before campaign finalization"',
)
campaign = campaign.replace(
    '"all nine provider records must produce campaign evidence"',
    '"all eleven provider records must produce campaign evidence"',
)
campaign = replace_once(campaign, ".finalize(campaign_evidence, 110)", ".finalize(campaign_evidence, 112)", "campaign final sequence")
campaign = replace_once(campaign, "(0, 9)", "(0, 11)", "RPOMR denominator")
campaign = replace_once(campaign, "observation_digests.len(), 9", "observation_digests.len(), 11", "observation digest count")
campaign = campaign.replace(
    '"clean measured W01-W09 L7 campaign may mint scoped provider pass"',
    '"clean measured W01-W09/W11/W12 L7 campaign may mint scoped provider pass"',
)
campaign = campaign.replace(
    '"serialize completed W01-W09 result"',
    '"serialize completed W01-W09/W11/W12 result"',
)
campaign_path.write_text(campaign, encoding="utf-8")

# 3. Make W11/W12 permanent Windows real-provider gates.
workflow_path = root / ".github/workflows/windows-real-provider-seeds.yml"
workflow = workflow_path.read_text(encoding="utf-8")
workflow = replace_once(
    workflow,
    "name: V4.3 L7 Windows real-provider seeds W01/W02/W03/W04/W05/W06/W07/W08/W09",
    "name: V4.3 L7 Windows real-provider seeds W01/W02/W03/W04/W05/W06/W07/W08/W09/W11/W12",
    "permanent job label",
)
workflow = replace_once(
    workflow,
    "- name: Build WPF edge seed for W03/W05/W07/W08/W09",
    "- name: Build WPF edge seed for W03/W05/W07/W08/W09/W11/W12",
    "edge seed build label",
)
old_campaign_step = '''      - name: Prospective nine-seed campaign binds exact W01 through W09 evidence
        shell: pwsh
        run: |
          $testName = "windows_l7_real_provider_campaign::prospective_l7_campaign_binds_w01_through_w09_to_exact_candidate"
          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_campaign -- --list
          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {
            throw "required bound nine-seed L7 campaign test is not registered: $testName"
          }
          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_campaign $testName -- --ignored --exact --nocapture --test-threads=1
'''
new_race_steps = '''      - name: W11 owned modal is blocked before any real input effect
        shell: pwsh
        run: |
          $testName = "windows_real_provider_w11::w11_modal_before_dispatch_is_blocked_before_any_real_input_effect"
          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w11 -- --list
          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {
            throw "required W11 real-provider test is not registered: $testName"
          }
          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w11 $testName -- --ignored --exact --nocapture --test-threads=1
      - name: W12 process restart rejects pre-restart authority before replacement effect
        shell: pwsh
        run: |
          $testName = "windows_real_provider_w12::w12_restart_rejects_pre_restart_authority_before_any_replacement_effect"
          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w12 -- --list
          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {
            throw "required W12 real-provider test is not registered: $testName"
          }
          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w12 $testName -- --ignored --exact --nocapture --test-threads=1
      - name: Prospective eleven-case campaign binds exact W01-W09 plus W11/W12 evidence
        shell: pwsh
        run: |
          $testName = "windows_l7_real_provider_campaign::prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate"
          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_campaign -- --list
          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {
            throw "required bound eleven-case L7 campaign test is not registered: $testName"
          }
          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_campaign $testName -- --ignored --exact --nocapture --test-threads=1
'''
workflow = replace_once(workflow, old_campaign_step, new_race_steps, "permanent W11/W12/campaign gates")
workflow_path.write_text(workflow, encoding="utf-8")

print("Applied bounded W11/W12 campaign + permanent-gate transformation.")
