from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement, found {count}")
    target.write_text(text.replace(old, new, 1))


replace_once(
    "crates/windows-uia-provider/src/event_buffer_lib.rs",
    '#[cfg(windows)]\npub use verified_input_windows::*;\n',
    '#[cfg(windows)]\npub use verified_input_windows::{\n    observe_windows_verified_input_context, snapshot_windows_keyboard_state,\n};\n#[cfg(windows)]\npub(crate) use verified_input_windows::windows_insert_verified_key_events;\n',
)
replace_once(
    "crates/windows-uia-provider/src/verified_input_windows.rs",
    "pub fn windows_insert_verified_key_events(\n",
    "pub(crate) fn windows_insert_verified_key_events(\n",
)

Path("crates/windows-uia-provider/tests/windows_verified_input_backend_contract.rs").write_text(
    '''#![cfg(windows)]

use localview_windows_uia_provider::{
    observe_windows_verified_input_context, snapshot_windows_keyboard_state,
};

#[test]
fn windows_read_only_backend_observation_surface_is_narrow_and_typed() {
    let _observe: fn(u64, u32, bool) -> _ = observe_windows_verified_input_context;
    let _snapshot: fn() -> _ = snapshot_windows_keyboard_state;
}
'''
)

Path(
    "tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_full_smoke.rs"
).write_text(
    '''use std::{
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
    let authority = mint_verified_input_authority(
        &attachment,
        snapshot.as_ref(),
        target.element_ref.clone(),
    )
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
'''
)

cases = Path(
    "tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_cases.rs"
)
text = cases.read_text()
old = "use std::{collections::BTreeSet, thread, time::{Duration, Instant}};\n"
new = 'use std::collections::BTreeSet;\n\n#[path = "v43_verified_input_full_smoke.rs"]\nmod full_smoke;\n'
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: unexpected std import")
text = text.replace(old, new, 1)
old = '''use localview_windows_uia_provider::{
    WindowsInputInsertRawResult, WindowsInputInsertionClass, WindowsKeyTransition,
    WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextBlocker,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements,
    WindowsUiaWorkerError, WindowsVerifiedInputBoundaryError, WindowsVerifiedInputEnvironment,
    WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch, execute_windows_verified_input_boundary,
    observe_windows_verified_input_context, snapshot_windows_keyboard_state,
    windows_insert_verified_key_events,
};
'''
new = '''use localview_windows_uia_provider::{
    WindowsInputInsertRawResult, WindowsInputInsertionClass, WindowsKeyTransition,
    WindowsKeyboardStateSnapshot, WindowsUiaDispatchContextBlocker,
    WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextRequirements,
    WindowsUiaWorkerError, WindowsVerifiedInputBoundaryError, WindowsVerifiedInputEnvironment,
    WindowsVerifiedKeyEvent, WindowsVerifiedKeyboardBatch, execute_windows_verified_input_boundary,
};
'''
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: unexpected provider import block")
text = text.replace(old, new, 1)
old = '''struct ProductionWindowsEnvironment {
    target_window_handle: u64,
    target_process_id: u32,
    insert_calls: u32,
}

impl WindowsVerifiedInputEnvironment for ProductionWindowsEnvironment {
    fn observe_dispatch_context(
        &mut self,
    ) -> Result<WindowsUiaDispatchContextObservation, WindowsVerifiedInputBoundaryError> {
        observe_windows_verified_input_context(
            self.target_window_handle,
            self.target_process_id,
            true,
        )
    }

    fn snapshot_keyboard_state(
        &mut self,
    ) -> Result<WindowsKeyboardStateSnapshot, WindowsVerifiedInputBoundaryError> {
        snapshot_windows_keyboard_state()
    }

    fn insert_events(&mut self, events: &[WindowsVerifiedKeyEvent]) -> WindowsInputInsertRawResult {
        self.insert_calls += 1;
        windows_insert_verified_key_events(events)
    }
}

'''
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: production raw environment block missing")
text = text.replace(old, "", 1)
old = '''fn two_event_space_batch() -> WindowsVerifiedKeyboardBatch {
    WindowsVerifiedKeyboardBatch::new(vec![
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyDown,
        },
        WindowsVerifiedKeyEvent {
            virtual_key: 0x20,
            transition: WindowsKeyTransition::KeyUp,
        },
    ])
    .expect("construct production W08 Space batch")
}

fn wait_for_effect(seed: &mut EdgeSeedProcess, timeout: Duration) -> serde_json::Value {
    let deadline = Instant::now() + timeout;
    loop {
        let state = seed.input_state();
        if truth_u64(&state, "effect_count") > 0 {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "W08 production full insertion reported success but WPF oracle observed no key effect"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

'''
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: raw full-smoke helpers missing")
text = text.replace(old, "", 1)
old = '''    let mut seed = EdgeSeedProcess::spawn();
    let fixture = seed.prepare_input_target();
    let target_window = truth_u64(&fixture, "window_handle");
    let target_process = truth_u64(&fixture, "process_id") as u32;
    assert!(truth_bool(&fixture, "target_is_foreground"));
    assert_eq!(truth_u64(&fixture, "effect_count"), 0);

    let mut production = ProductionWindowsEnvironment {
        target_window_handle: target_window,
        target_process_id: target_process,
        insert_calls: 0,
    };
    let full = execute_windows_verified_input_boundary(
        context_requirements(),
        &two_event_space_batch(),
        &mut production,
    )
    .expect("campaign W08 production SendInput smoke must traverse verified receipt path");
    assert_eq!(production.insert_calls, 1);
    assert_eq!(full.requested_event_count, 2);
    assert_eq!(full.inserted_event_count, 2);
    assert_eq!(full.insertion_class, WindowsInputInsertionClass::FullyInserted);
    assert!(full.reconciliation_required);
    let after = wait_for_effect(&mut seed, Duration::from_secs(2));
    assert_eq!(truth_u64(&after, "effect_count"), 1);
    assert!(truth_bool(&after, "target_is_foreground"));
'''
new = '''    let mut seed = EdgeSeedProcess::spawn();
    let full = full_smoke::run_production_verified_input_full_smoke(
        &mut seed,
        "cut:v43:campaign:w08:production-full",
    )
    .await;
    assert_eq!(full.requested_event_count, 2);
    assert_eq!(full.inserted_event_count, 2);
    assert_eq!(full.insertion_class, WindowsInputInsertionClass::FullyInserted);
    assert!(full.reconciliation_required);
    assert_eq!(full.dispatch_result, DispatchResult::DispatchedFull);
    assert_eq!(full.effect_count, 1);
    assert!(full.target_is_foreground);
'''
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: W08 production block missing")
text = text.replace(old, new, 1)
old = '''        "production_windows_full_smoke": {
            "requested_event_count": full.requested_event_count,
            "inserted_event_count": full.inserted_event_count,
            "classification": "fully_inserted",
            "reconciliation_required": full.reconciliation_required,
            "insert_call_count": production.insert_calls,
            "oracle_effect_count": truth_u64(&after, "effect_count")
        },
        "claim_boundary": "partial-count evidence is deterministic wrapper/property evidence; hosted Windows production evidence proves ordinary full insertion through the same verified receipt boundary"
'''
new = '''        "production_windows_full_smoke": {
            "requested_event_count": full.requested_event_count,
            "inserted_event_count": full.inserted_event_count,
            "classification": "fully_inserted",
            "reconciliation_required": full.reconciliation_required,
            "dispatch_result": "dispatched_full",
            "authority_path": "journal-minted-request-to-exact-uia-worker",
            "oracle_effect_count": full.effect_count
        },
        "claim_boundary": "partial-count evidence is deterministic wrapper/property evidence; hosted Windows production evidence proves ordinary full insertion only through journal-minted exact UIA worker authority"
'''
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: W08 ground-truth block missing")
text = text.replace(old, new, 1)
old = '            "windows-uia:w08:production-sendinput-full:requested=2,inserted=2".into(),\n'
new = '            "windows-uia:w08:production-worker-full:requested=2,inserted=2".into(),\n            "windows-uia:w08:production-authority-path:journal-minted-exact-uia-worker".into(),\n'
if text.count(old) != 1:
    raise SystemExit("verified_input_cases: W08 production evidence ref missing")
text = text.replace(old, new, 1)
cases.write_text(text)

w08 = Path("tools/validation-lab/windows-l7-real-provider-harness/tests/v43_real_provider_w08.rs")
text = w08.read_text()
prefix = '''#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
#[path = "support/v43_verified_input_full_smoke.rs"]
mod verified_input_full_smoke;

'''
if not text.startswith(prefix):
    text = prefix + text
full_test = '''

#[cfg(windows)]
mod windows_real_provider_w08_full {
    use std::{fs, path::PathBuf};

    use localview_protocol::DispatchResult;
    use localview_windows_uia_provider::WindowsInputInsertionClass;
    use serde_json::json;

    use super::{
        verified_input_full_smoke::run_production_verified_input_full_smoke,
        verified_input_seed::EdgeSeedProcess,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires interactive Windows desktop and WPF edge seed"]
    async fn w08_production_worker_full_dispatch_uses_authority_path() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real production input smoke must be explicitly enabled"
        );

        let mut seed = EdgeSeedProcess::spawn();
        let full = run_production_verified_input_full_smoke(
            &mut seed,
            "cut:v43:w08:production-worker-full",
        )
        .await;
        assert_eq!(full.requested_event_count, 2);
        assert_eq!(full.inserted_event_count, 2);
        assert_eq!(full.insertion_class, WindowsInputInsertionClass::FullyInserted);
        assert!(full.reconciliation_required);
        assert_eq!(full.dispatch_result, DispatchResult::DispatchedFull);
        assert_eq!(full.effect_count, 1);
        assert!(full.target_is_foreground);

        if let Some(dir) = std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR") {
            let path = PathBuf::from(dir).join("W08-PRODUCTION-FULL-SMOKE.json");
            let artifact = json!({
                "case_id": "W08-partial-input-dispatch",
                "evidence_kind": "production-windows-worker-full-dispatch-smoke",
                "candidate_sha": std::env::var("LOCALVIEW_CANDIDATE_SHA")
                    .unwrap_or_else(|_| "unknown:standalone-w08-smoke".into()),
                "requested_event_count": full.requested_event_count,
                "inserted_event_count": full.inserted_event_count,
                "insertion_class": "fully-inserted",
                "dispatch_result": "dispatched-full",
                "reconciliation_required": full.reconciliation_required,
                "authority_path": "journal-minted WindowsUiaVerifiedInputRequest -> WindowsUiaWorker::dispatch_verified_input",
                "independent_oracle_effect_count": full.effect_count,
                "natural_windows_partial_observed": false,
                "claim_boundary": "production backend evidence proves a full dispatch only through the authority-gated worker; it does not claim hosted Windows naturally produced a partial SendInput result"
            });
            fs::create_dir_all(path.parent().expect("artifact parent"))
                .expect("create W08 artifact directory");
            fs::write(
                path,
                serde_json::to_vec_pretty(&artifact)
                    .expect("serialize W08 production worker smoke artifact"),
            )
            .expect("persist W08 production worker smoke artifact");
        }

        seed.shutdown();
    }
}
'''
if "w08_production_worker_full_dispatch_uses_authority_path" not in text:
    text += full_test
w08.write_text(text)

old_smoke = Path("crates/windows-uia-provider/tests/windows_verified_input_smoke.rs")
if old_smoke.exists():
    old_smoke.unlink()

plan = Path("docs/superpowers/plans/2026-09-12-v43-windows-verified-input-w07-w08-w09.md")
text = plan.read_text()
text = text.replace(
    '- Create/modify: `crates/windows-uia-provider/tests/windows_verified_input_smoke.rs`.\n',
    '- Create/modify: `tools/validation-lab/windows-l7-real-provider-harness/tests/support/v43_verified_input_full_smoke.rs`.\n',
)
text = text.replace(
    '- [x] Separate Windows smoke proves the production backend uses the same receipt path for an ordinary full dispatch against synthetic target.\n',
    '- [x] Separate Windows smoke proves the production backend only through a journal-minted request and exact UIA worker for an ordinary full dispatch against synthetic target.\n',
)
plan.write_text(text)
