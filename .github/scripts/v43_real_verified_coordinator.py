from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
path = root / "crates/windows-observe-runtime/tests/windows_runtime_dispatch_smoke.rs"
text = path.read_text()

text = text.replace(
'''        ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n        ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,\n        ConsequentialRecoveryState, LiveBridge, reconcile_consequential_postconditions,\n''',
'''        ConsequentialJournal, ConsequentialJournalTransition, ConsequentialPostconditionEvidence,\n        ConsequentialPostconditionStatus, ConsequentialRecoveryState, LiveBridge,\n''', 1)
text = text.replace(
'''    use localview_native_provider::{SnapshotBudget, UserSelectedWindowTarget};\n    use localview_protocol::{DispatchResult, PrincipalRef, TransportResult, WorldOutcome};\n''',
'''    use localview_native_provider::{\n        NativeSemanticSnapshotRevision, SnapshotBudget, UserSelectedWindowTarget,\n    };\n    use localview_protocol::{PrincipalRef, WorldOutcome};\n''', 1)
text = text.replace(
'''        WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,\n        WindowsUiaDispatchSealRequest, WindowsUiaPreparedDispatchRequest,\n        arm_uia_dispatch_execution, execute_armed_uia_dispatch, prepare_uia_dispatch,\n        spawn_windows_uia_runtime_manager,\n''',
'''        WindowsUiaAuthorizationRevalidationReceipt, WindowsUiaAuthorizationRevalidator,\n        WindowsUiaDispatchSealRequest, WindowsUiaPostconditionVerifier,\n        WindowsUiaPreparedDispatchRequest, WindowsUiaVerifiedExecutionOutcome,\n        arm_uia_dispatch_execution, execute_armed_uia_dispatch_verified, prepare_uia_dispatch,\n        spawn_windows_uia_runtime_manager,\n''', 1)

anchor = '''    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]\n'''
verifier = r'''    struct SmokePostconditionVerifier {
        pre_dispatch_cut: String,
    }

    impl WindowsUiaPostconditionVerifier for SmokePostconditionVerifier {
        type Error = Infallible;

        fn verify(
            &self,
            _action_id: Uuid,
            expected_contract_refs: &[String],
            snapshot: &NativeSemanticSnapshotRevision,
        ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
            assert_eq!(expected_contract_refs, &[POSTCONDITION_REF.to_owned()]);
            assert_ne!(
                snapshot.snapshot_cut_ref(),
                self.pre_dispatch_cut,
                "production coordinator must verify a fresh post-dispatch cut"
            );
            let title_changed = snapshot
                .nodes()
                .iter()
                .any(|node| node.name.as_deref() == Some(AFTER_TITLE));
            Ok(vec![ConsequentialPostconditionEvidence {
                contract_ref: POSTCONDITION_REF.into(),
                status: if title_changed {
                    ConsequentialPostconditionStatus::VerifiedPass
                } else {
                    ConsequentialPostconditionStatus::VerifiedFail
                },
                receipt_ref: format!(
                    "postcondition:runtime-smoke:{}",
                    snapshot.cache_revision_ref()
                ),
            }])
        }
    }

'''
if anchor not in text:
    raise SystemExit("test anchor missing")
text = text.replace(anchor, verifier + anchor, 1)

start = '''        let result = execute_armed_uia_dispatch(&bridge, &journal, session_id, armed, &executor)\n            .await\n            .expect("real retained UIA Invoke must cross the one-shot runtime boundary");\n'''
end = '''        assert_eq!(\n            journal.recovery_state(action_id).await,\n            Some(ConsequentialRecoveryState::Committed)\n        );\n'''
if start not in text or end not in text:
    raise SystemExit("manual verified flow anchors missing")
start_index = text.index(start)
end_index = text.index(end, start_index) + len(end)
replacement = r'''        let outcome = execute_armed_uia_dispatch_verified(
            &bridge,
            &journal,
            &manager,
            session_id,
            armed,
            &executor,
            &SmokePostconditionVerifier {
                pre_dispatch_cut: snapshot.snapshot_cut_ref().to_owned(),
            },
        )
        .await
        .expect("production verified executor must close real Invoke through postcondition commit");

        assert!(matches!(
            outcome,
            WindowsUiaVerifiedExecutionOutcome::Committed {
                action_id: committed_action_id,
                world_outcome: WorldOutcome::VerifiedExpected,
                ..
            } if committed_action_id == action_id
        ));
        assert_eq!(
            journal.recovery_state(action_id).await,
            Some(ConsequentialRecoveryState::Committed)
        );
        let entries = journal.entries_for(action_id).await;
        let dispatch_sequence = entries
            .iter()
            .find(|entry| matches!(
                entry.transition,
                ConsequentialJournalTransition::DispatchLinearized { .. }
            ))
            .map(|entry| entry.journal_sequence)
            .expect("real provider dispatch must be durably linearized");
        let reconciliation_sequence = entries
            .iter()
            .find(|entry| matches!(
                entry.transition,
                ConsequentialJournalTransition::ReconciliationOutcome {
                    world_outcome: WorldOutcome::VerifiedExpected,
                    postconditions_verified: true,
                    ..
                }
            ))
            .map(|entry| entry.journal_sequence)
            .expect("fresh postcondition evidence must be durably reconciled");
        let commit_sequence = entries
            .iter()
            .find(|entry| matches!(entry.transition, ConsequentialJournalTransition::Committed))
            .map(|entry| entry.journal_sequence)
            .expect("verified expected world outcome must be durably committed");
        assert!(dispatch_sequence < reconciliation_sequence);
        assert!(reconciliation_sequence < commit_sequence);
'''
text = text[:start_index] + replacement + text[end_index:]
path.write_text(text)

subprocess.run(["cargo", "fmt", "--all"], cwd=root, check=True)
subprocess.run(["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"], cwd=root, check=True)
subprocess.run(["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior"], cwd=root, check=True)
subprocess.run(["git", "diff", "--check"], cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run(["git", "add", "crates/windows-observe-runtime/tests/windows_runtime_dispatch_smoke.rs"], cwd=root, check=True)
subprocess.run(["git", "rm", "-f", ".github/scripts/v43_real_verified_coordinator.py", ".github/workflows/v43-real-verified-coordinator.yml"], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "test(v43): route real UIA commit through verified coordinator"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-consequential-verified-execution-coordinator"], cwd=root, check=True)
