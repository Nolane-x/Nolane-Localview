from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"
text = path.read_text()

anchor = '''#[tokio::test]\nasync fn exact_provider_receipt_is_durably_linearized_before_returning_success() {\n'''
test = r'''#[tokio::test]
async fn stale_canonical_authority_before_executor_releases_live_execution_grant() {
    let (bridge, journal, path, _provider, _runtime, armed) =
        verified_prepared_and_armed("stale-canonical-before-executor").await;
    let action_id = armed.action_id();
    assert!(
        bridge.release_provider_observation(session()).await,
        "test must invalidate the provider-bound canonical freshness after arming"
    );
    let executor = FakeExecutor::new(ExecutorMode::Dispatched);

    let error = execute_armed_uia_dispatch(&bridge, &journal, session(), armed, &executor)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WindowsUiaDispatchExecutionCoordinatorError::CanonicalEnvelopeStaleBeforeExecutor
    ));
    assert_eq!(
        executor.call_count(),
        0,
        "stale canonical authority must fail before provider execution"
    );
    assert_eq!(
        journal.recovery_state(action_id).await,
        Some(ConsequentialRecoveryState::DispatchPrepared)
    );

    let observation = journal
        .begin_postcondition_observation(action_id)
        .await
        .expect("pre-executor rejection must release live execution authority for reconciliation");
    journal
        .abandon_postcondition_observation(observation)
        .await
        .unwrap();

    let _ = std::fs::remove_file(path);
}

'''
if "stale_canonical_authority_before_executor_releases_live_execution_grant" in text:
    raise SystemExit("RED test already present")
if anchor not in text:
    raise SystemExit("test insertion anchor missing")
text = text.replace(anchor, test + anchor, 1)
path.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)
subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_preexecutor_abandon_red.py", ".github/workflows/v43-preexecutor-abandon-red.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): require pre-executor authority abandonment"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
