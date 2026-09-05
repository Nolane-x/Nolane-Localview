from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
source = root / "crates/live-bridge/src/consequential_journal.rs"
text = source.read_text()
marker = "    /// Commit the executor/dispatch receipt using the exact one-shot permit.\n"
if "pub async fn abandon_dispatch_execution" not in text:
    method = r'''    /// Abandon one exact live execution permit without changing durable recovery state.
    ///
    /// This is the fail-closed counterpart to `record_dispatch_linearized` for
    /// executor/receipt failures that occur after `begin_dispatch` consumed the
    /// PREPARED capability but before a durable dispatch receipt exists. The
    /// exact live grant is removed and never recreated; durable PREPARED remains
    /// authoritative and therefore requires reconciliation rather than retry.
    pub async fn abandon_dispatch_execution(
        &self,
        permit: DispatchExecutionPermit,
    ) -> Result<(), ConsequentialJournalError> {
        let action_id = permit.action_id;
        if permit.journal_instance_ref != self.journal_instance_ref {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "journal_instance_mismatch",
            });
        }

        let mut state = self.state.lock().await;
        let Some(grant) = state.live_dispatch_execution.get(&action_id) else {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "live_execution_grant_missing",
            });
        };
        if grant.permit_ref != permit.permit_ref
            || grant.preparation_journal_sequence != permit.preparation_journal_sequence
            || grant.preparation_receipt_ref != permit.preparation_receipt_ref
        {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "execution_grant_binding_mismatch",
            });
        }

        // Consume first and never restore it. Any later validation failure is
        // still fail-closed: the durable journal remains the recovery authority.
        state.live_dispatch_execution.remove(&action_id);

        let current = recovery_state_for(&state.entries, action_id);
        if current != Some(ConsequentialRecoveryState::DispatchPrepared) {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "durable_state_is_not_prepared",
            });
        }
        if !durable_preparation_matches(
            &state.entries,
            action_id,
            permit.preparation_journal_sequence,
            &permit.preparation_receipt_ref,
        ) {
            return Err(ConsequentialJournalError::InvalidDispatchPermit {
                action_id,
                reason: "durable_prepared_entry_mismatch",
            });
        }

        Ok(())
    }

'''
    if marker not in text:
        raise SystemExit("insertion marker missing")
    text = text.replace(marker, method + marker, 1)
    source.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_execution_abandonment"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_post_dispatch_observation_authority"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_dispatch_capability"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/live-bridge/src/consequential_journal.rs", "crates/live-bridge/tests/v43_dispatch_execution_abandonment.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_dispatch_abandon_green.py", ".github/workflows/v43-dispatch-abandon-green.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "feat(v43): abandon live dispatch execution authority fail closed"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
