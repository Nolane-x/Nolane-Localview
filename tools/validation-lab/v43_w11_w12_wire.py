from pathlib import Path


def once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    return text.replace(old, new, 1)


campaign_path = Path("tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_campaign.rs")
c = campaign_path.read_text(encoding="utf-8")
c = once(c, '#[path = "support/v43_verified_input_cases.rs"]\nmod verified_input_cases;\n', '#[path = "support/v43_verified_input_cases.rs"]\nmod verified_input_cases;\n\n#[cfg(windows)]\n#[path = "support/v43_verified_input_cases_w11_w12.rs"]\nmod verified_input_cases_w11_w12;\n', "campaign module")
c = once(c, 'use super::{baseline_cases, follow_on_cases, verified_input_cases};', 'use super::{\n        baseline_cases, follow_on_cases, verified_input_cases, verified_input_cases_w11_w12,\n    };', "campaign use")
c = once(c, 'const REQUIRED_CASES: [&str; 9] = [', 'const REQUIRED_CASES: [&str; 11] = [', "case count")
c = once(c, '        "W09-user-held-modifier-interference",\n    ];', '        "W09-user-held-modifier-interference",\n        "W11-modal-before-dispatch",\n        "W12-target-restart-after-authorization",\n    ];', "case ids")
c = once(c, 'async fn prospective_l7_campaign_binds_w01_through_w09_to_exact_candidate()', 'async fn prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate()', "test name")
c = c.replace('lab-v43-windows-l7-r3', 'lab-v43-windows-l7-r4')
c = c.replace('windows-provider-seeds-w01-w09-r3', 'windows-provider-seeds-w01-w09-w11-w12-r4')
c = c.replace('digest canonical nine-seed environment manifest', 'digest canonical 11-case environment manifest')
c = c.replace('digest prospective W01-W09 L7 seed catalog', 'digest prospective W01-W09/W11/W12 L7 seed catalog')
c = c.replace('prepare nine-seed L7 preregistration', 'prepare 11-case L7 preregistration')
c = c.replace('start typed prospective W01-W09 provider campaign', 'start typed prospective W01-W09/W11/W12 provider campaign')
c = once(c, '''                LabSeedIdentity {\n                    seed_id: REQUIRED_CASES[8].into(),\n                    prediction_revision: "w09-modifier-interference-r1".into(),\n                    oracle_revision: "independent-wpf-input-oracle-r1".into(),\n                },\n            ],''', '''                LabSeedIdentity {\n                    seed_id: REQUIRED_CASES[8].into(),\n                    prediction_revision: "w09-modifier-interference-r1".into(),\n                    oracle_revision: "independent-wpf-input-oracle-r1".into(),\n                },\n                LabSeedIdentity {\n                    seed_id: REQUIRED_CASES[9].into(),\n                    prediction_revision: "w11-owned-modal-final-fence-r1".into(),\n                    oracle_revision: "independent-wpf-modal-oracle-r1".into(),\n                },\n                LabSeedIdentity {\n                    seed_id: REQUIRED_CASES[10].into(),\n                    prediction_revision: "w12-target-restart-invalidates-authority-r1".into(),\n                    oracle_revision: "independent-process-lifecycle-plus-wpf-input-oracle-r1".into(),\n                },\n            ],''', "seed identities")
c = once(c, '                "user-held modifier != LocalView normalization authority".into(),\n            ]),', '                "user-held modifier != LocalView normalization authority".into(),\n                "clean preflight target != authority to cross an owned modal final fence".into(),\n                "logical target resemblance after restart != stale process-A dispatch authority".into(),\n            ]),', "distinctions")
c = once(c, '''            verified_input_cases::run_w09(\n                &edge_seed_digest,\n                &environment_digest_text,\n                PLATFORM_PROFILE,\n                COMPARISON_PROFILE,\n                109,\n            )\n            .await,\n        ];''', '''            verified_input_cases::run_w09(\n                &edge_seed_digest,\n                &environment_digest_text,\n                PLATFORM_PROFILE,\n                COMPARISON_PROFILE,\n                109,\n            )\n            .await,\n            verified_input_cases_w11_w12::run_w11(\n                &edge_seed_digest,\n                &environment_digest_text,\n                PLATFORM_PROFILE,\n                COMPARISON_PROFILE,\n                110,\n            )\n            .await,\n            verified_input_cases_w11_w12::run_w12(\n                &edge_seed_digest,\n                &environment_digest_text,\n                PLATFORM_PROFILE,\n                COMPARISON_PROFILE,\n                111,\n            )\n            .await,\n        ];''', "records")
c = c.replace('the prospective campaign must execute exactly W01 through W09', 'the prospective campaign must execute exactly W01-W09 plus W11 and W12; W10 remains unmeasured')
c = c.replace('every required W01-W09 case must be complete before campaign finalization', 'every required W01-W09/W11/W12 case must be complete before campaign finalization')
c = c.replace('all nine provider records must produce campaign evidence', 'all 11 provider records must produce campaign evidence')
c = c.replace('clean measured W01-W09 L7 campaign may mint scoped provider pass', 'clean measured W01-W09/W11/W12 L7 campaign may mint scoped provider pass')
c = once(c, '.finalize(campaign_evidence, 110)', '.finalize(campaign_evidence, 112)', "final sequence")
c = once(c, 'assert_eq!((rpomr.numerator, rpomr.denominator), (0, 9));', 'assert_eq!((rpomr.numerator, rpomr.denominator), (0, 11));', "RPOMR")
c = once(c, 'assert_eq!(completed.payload.observation_digests.len(), 9);', 'assert_eq!(completed.payload.observation_digests.len(), 11);', "digests")
c = c.replace('serialize completed W01-W09 result', 'serialize completed W01-W09/W11/W12 result')
c = c.replace('persist completed nine-seed L7 result artifact', 'persist completed 11-case L7 result artifact')
for required in ['"W11-modal-before-dispatch"', '"W12-target-restart-after-authorization"', '(0, 11)', 'observation_digests.len(), 11']:
    if required not in c:
        raise SystemExit(f"campaign postcondition missing: {required}")
campaign_path.write_text(c, encoding="utf-8")

workflow_path = Path(".github/workflows/windows-real-provider-seeds.yml")
w = workflow_path.read_text(encoding="utf-8")
w = once(w, 'V4.3 L7 Windows real-provider seeds W01/W02/W03/W04/W05/W06/W07/W08/W09', 'V4.3 L7 Windows real-provider seeds W01/W02/W03/W04/W05/W06/W07/W08/W09/W11/W12', "job name")
w = once(w, 'Build WPF edge seed for W03/W05/W07/W08/W09', 'Build WPF edge seed for W03/W05/W07/W08/W09/W11/W12', "seed label")
anchor = '      - name: Prospective nine-seed campaign binds exact W01 through W09 evidence\n'
if w.count(anchor) != 1:
    raise SystemExit("campaign workflow anchor mismatch")
steps = '''      - name: W11 owned modal is blocked at the final worker input fence\n        shell: pwsh\n        run: |\n          $testName = "windows_real_provider_w11::w11_modal_before_dispatch_is_blocked_before_any_real_input_effect"\n          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w11 -- --list\n          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {\n            throw "required W11 real-provider test is not registered: $testName"\n          }\n          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w11 $testName -- --ignored --exact --nocapture --test-threads=1\n      - name: W12 process restart invalidates pre-restart input authority\n        shell: pwsh\n        run: |\n          $testName = "windows_real_provider_w12::w12_restart_rejects_pre_restart_authority_before_any_replacement_effect"\n          $listed = cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w12 -- --list\n          if (-not ($listed | Select-String -SimpleMatch "${testName}: test")) {\n            throw "required W12 real-provider test is not registered: $testName"\n          }\n          cargo test --manifest-path $env:LOCALVIEW_L7_HARNESS_MANIFEST --test v43_real_provider_w12 $testName -- --ignored --exact --nocapture --test-threads=1\n'''
w = w.replace(anchor, steps + anchor, 1)
w = w.replace('Prospective nine-seed campaign binds exact W01 through W09 evidence', 'Prospective 11-case campaign binds exact W01-W09 plus W11/W12 evidence')
w = w.replace('windows_l7_real_provider_campaign::prospective_l7_campaign_binds_w01_through_w09_to_exact_candidate', 'windows_l7_real_provider_campaign::prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate')
w = w.replace('required bound nine-seed L7 campaign test is not registered', 'required bound 11-case L7 campaign test is not registered')
for required in ['W11 owned modal', 'W12 process restart', 'prospective_l7_campaign_binds_w01_w09_w11_w12_to_exact_candidate']:
    if required not in w:
        raise SystemExit(f"workflow postcondition missing: {required}")
workflow_path.write_text(w, encoding="utf-8")

print("wired W11/W12 into exact 11-case campaign and permanent Windows gates")
