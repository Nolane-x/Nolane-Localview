from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[2]
journal_path = root / "crates/live-bridge/src/consequential_journal.rs"
reconcile_path = root / "crates/live-bridge/src/postcondition_reconciliation.rs"

journal = journal_path.read_text()
reconcile = reconcile_path.read_text()


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"{label}: expected exactly one anchor, found {text.count(old)}")
    return text.replace(old, new, 1)

journal = replace_once(
    journal,
    "    collections::HashMap,\n",
    "    collections::{BTreeSet, HashMap},\n",
    "collections import",
)

journal = replace_once(
    journal,
    '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct DispatchLinearizationReceipt {\n    pub receipt_ref: String,\n    pub transport_result: TransportResult,\n    pub dispatch_result: DispatchResult,\n}\n\n''',
    '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct DispatchLinearizationReceipt {\n    pub receipt_ref: String,\n    pub transport_result: TransportResult,\n    pub dispatch_result: DispatchResult,\n}\n\n#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]\n#[serde(rename_all = "snake_case")]\npub enum ActionPostconditionVerdict {\n    VerifiedExpected,\n    VerifiedUnexpected,\n    ReconciliationRequired,\n}\n\nimpl ActionPostconditionVerdict {\n    pub fn world_outcome(self) -> WorldOutcome {\n        match self {\n            Self::VerifiedExpected => WorldOutcome::VerifiedExpected,\n            Self::VerifiedUnexpected => WorldOutcome::VerifiedUnexpected,\n            Self::ReconciliationRequired => WorldOutcome::ReconciliationRequired,\n        }\n    }\n\n    pub fn postconditions_verified(self) -> bool {\n        self == Self::VerifiedExpected\n    }\n}\n\n/// First-class durable proof of independently observed action postconditions.\n///\n/// Receipt identity and completion ordering are journal-minted. Contract\n/// classifications form an exact partition of the admitted expected contracts,\n/// while causal assurance binds the observation to durable dispatch history.\n#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub struct ActionPostconditionReceipt {\n    pub receipt_ref: String,\n    pub action_id: Uuid,\n    pub session_id: SessionId,\n    pub provider_incarnation_ref: ProviderIncarnationRef,\n    pub target_incarnation_ref: TargetIncarnationRef,\n    pub expected_postcondition_contract_refs: Vec<String>,\n    pub observation_snapshot_cut_ref: String,\n    pub reconciliation_receipt_ref: String,\n    pub evidence_receipt_refs: Vec<String>,\n    pub verified_contract_refs: Vec<String>,\n    pub failed_contract_refs: Vec<String>,\n    pub verdict: ActionPostconditionVerdict,\n    pub unresolved_unknown_contract_refs: Vec<String>,\n    pub causal_assurance: ConsequentialPostconditionObservationCause,\n    pub completion_journal_sequence: u64,\n}\n\n#[derive(Debug, Clone)]\npub(crate) struct ActionPostconditionReceiptDraft {\n    pub action_id: Uuid,\n    pub session_id: SessionId,\n    pub provider_incarnation_ref: ProviderIncarnationRef,\n    pub target_incarnation_ref: TargetIncarnationRef,\n    pub expected_postcondition_contract_refs: Vec<String>,\n    pub observation_snapshot_cut_ref: String,\n    pub reconciliation_receipt_ref: String,\n    pub evidence_receipt_refs: Vec<String>,\n    pub verified_contract_refs: Vec<String>,\n    pub failed_contract_refs: Vec<String>,\n    pub verdict: ActionPostconditionVerdict,\n    pub unresolved_unknown_contract_refs: Vec<String>,\n    pub causal_assurance: ConsequentialPostconditionObservationCause,\n}\n\n''',
    "durable postcondition types",
)

journal = replace_once(
    journal,
    '''    DispatchLinearized {\n        receipt: DispatchLinearizationReceipt,\n    },\n    ReconciliationOutcome {\n''',
    '''    DispatchLinearized {\n        receipt: DispatchLinearizationReceipt,\n    },\n    PostconditionReceiptRecorded {\n        receipt: ActionPostconditionReceipt,\n    },\n    /// Historical aggregate transition retained strictly for replay compatibility.\n    /// New postcondition outcomes must use `PostconditionReceiptRecorded`.\n    ReconciliationOutcome {\n''',
    "transition variant",
)

journal = replace_once(
    journal,
    '''#[derive(Debug, Clone, PartialEq, Eq)]\npub enum ConsequentialPostconditionObservationCause {\n''',
    '''#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]\npub enum ConsequentialPostconditionObservationCause {\n''',
    "cause serde",
)

journal = replace_once(
    journal,
    '''impl ConsequentialPostconditionObservationCause {\n    fn causal_journal_sequence(&self) -> u64 {\n''',
    '''impl ConsequentialPostconditionObservationCause {\n    pub fn causal_journal_sequence(&self) -> u64 {\n''',
    "public causal sequence",
)

journal = replace_once(
    journal,
    '''    pub async fn recovery_state(&self, action_id: Uuid) -> Option<ConsequentialRecoveryState> {\n        let state = self.state.lock().await;\n        recovery_state_for(&state.entries, action_id)\n    }\n\n''',
    '''    pub async fn recovery_state(&self, action_id: Uuid) -> Option<ConsequentialRecoveryState> {\n        let state = self.state.lock().await;\n        recovery_state_for(&state.entries, action_id)\n    }\n\n    pub async fn latest_action_postcondition_receipt(\n        &self,\n        action_id: Uuid,\n    ) -> Option<ActionPostconditionReceipt> {\n        self.state\n            .lock()\n            .await\n            .entries\n            .iter()\n            .rev()\n            .find_map(|entry| {\n                if entry.action_id != action_id {\n                    return None;\n                }\n                match &entry.transition {\n                    ConsequentialJournalTransition::PostconditionReceiptRecorded { receipt } => {\n                        Some(receipt.clone())\n                    }\n                    _ => None,\n                }\n            })\n    }\n\n''',
    "latest receipt accessor",
)

journal = replace_once(
    journal,
    '''    pub(crate) async fn record_reconciliation_outcome(\n        &self,\n        action_id: Uuid,\n        world_outcome: WorldOutcome,\n        reconciliation_receipt_ref: Option<String>,\n        postcondition_receipt_refs: Vec<String>,\n        postconditions_verified: bool,\n    ) -> Result<ConsequentialJournalEntry, ConsequentialJournalError> {\n        self.append_validated(\n            action_id,\n            ConsequentialJournalTransition::ReconciliationOutcome {\n                world_outcome,\n                reconciliation_receipt_ref,\n                postcondition_receipt_refs,\n                postconditions_verified,\n            },\n        )\n        .await\n    }\n\n''',
    '''    pub(crate) async fn record_action_postcondition_receipt(\n        &self,\n        draft: ActionPostconditionReceiptDraft,\n    ) -> Result<(ConsequentialJournalEntry, ActionPostconditionReceipt), ConsequentialJournalError> {\n        let action_id = draft.action_id;\n        let mut state = self.state.lock().await;\n        let completion_journal_sequence = state.next_sequence;\n        let receipt = ActionPostconditionReceipt {\n            receipt_ref: format!(\n                "postcondition:{action_id}:{completion_journal_sequence}"\n            ),\n            action_id,\n            session_id: draft.session_id,\n            provider_incarnation_ref: draft.provider_incarnation_ref,\n            target_incarnation_ref: draft.target_incarnation_ref,\n            expected_postcondition_contract_refs: draft.expected_postcondition_contract_refs,\n            observation_snapshot_cut_ref: draft.observation_snapshot_cut_ref,\n            reconciliation_receipt_ref: draft.reconciliation_receipt_ref,\n            evidence_receipt_refs: draft.evidence_receipt_refs,\n            verified_contract_refs: draft.verified_contract_refs,\n            failed_contract_refs: draft.failed_contract_refs,\n            verdict: draft.verdict,\n            unresolved_unknown_contract_refs: draft.unresolved_unknown_contract_refs,\n            causal_assurance: draft.causal_assurance,\n            completion_journal_sequence,\n        };\n        let entry = self\n            .append_validated_locked(\n                &mut state,\n                action_id,\n                ConsequentialJournalTransition::PostconditionReceiptRecorded {\n                    receipt: receipt.clone(),\n                },\n            )\n            .await?;\n        Ok((entry, receipt))\n    }\n\n''',
    "record durable receipt",
)

journal = replace_once(
    journal,
    '''        ConsequentialJournalTransition::ReconciliationOutcome { .. } => match current {\n            None => Err(ConsequentialJournalError::UnknownAction { action_id }),\n            Some(ConsequentialRecoveryState::DispatchPrepared)\n            | Some(ConsequentialRecoveryState::PossiblyDispatched)\n            | Some(ConsequentialRecoveryState::KnownNotDispatched)\n            | Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => Ok(()),\n            _ => Err(ConsequentialJournalError::InvalidTransition {\n                action_id,\n                attempted: "reconciliation_outcome",\n                current,\n            }),\n        },\n''',
    '''        ConsequentialJournalTransition::PostconditionReceiptRecorded { receipt } => {\n            if current.is_none() {\n                return Err(ConsequentialJournalError::UnknownAction { action_id });\n            }\n            if !matches!(\n                current,\n                Some(ConsequentialRecoveryState::DispatchPrepared)\n                    | Some(ConsequentialRecoveryState::PossiblyDispatched)\n                    | Some(ConsequentialRecoveryState::OutcomeObservedUnverified)\n            ) {\n                return Err(ConsequentialJournalError::InvalidTransition {\n                    action_id,\n                    attempted: "postcondition_receipt_recorded",\n                    current,\n                });\n            }\n            validate_action_postcondition_receipt(entries, action_id, receipt, current)\n        }\n        ConsequentialJournalTransition::ReconciliationOutcome { .. } => {\n            if mode != TransitionValidationMode::Replay {\n                return Err(ConsequentialJournalError::InvalidTransition {\n                    action_id,\n                    attempted: "legacy_reconciliation_outcome_append_forbidden",\n                    current,\n                });\n            }\n            match current {\n                None => Err(ConsequentialJournalError::UnknownAction { action_id }),\n                Some(ConsequentialRecoveryState::DispatchPrepared)\n                | Some(ConsequentialRecoveryState::PossiblyDispatched)\n                | Some(ConsequentialRecoveryState::KnownNotDispatched)\n                | Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => Ok(()),\n                _ => Err(ConsequentialJournalError::InvalidTransition {\n                    action_id,\n                    attempted: "reconciliation_outcome",\n                    current,\n                }),\n            }\n        }\n''',
    "transition validation",
)

validation_helper = r'''
fn validate_action_postcondition_receipt(
    entries: &[ConsequentialJournalEntry],
    action_id: Uuid,
    receipt: &ActionPostconditionReceipt,
    current: Option<ConsequentialRecoveryState>,
) -> Result<(), ConsequentialJournalError> {
    let invalid = |attempted: &'static str| ConsequentialJournalError::InvalidTransition {
        action_id,
        attempted,
        current,
    };
    let envelope = admitted_envelope_for(entries, action_id)
        .ok_or(ConsequentialJournalError::UnknownAction { action_id })?;

    if receipt.action_id != action_id {
        return Err(invalid("postcondition_receipt_action_mismatch"));
    }
    if receipt.session_id != envelope.session_id {
        return Err(invalid("postcondition_receipt_session_mismatch"));
    }
    if receipt.provider_incarnation_ref != envelope.metadata.provider_incarnation_ref {
        return Err(invalid("postcondition_receipt_provider_mismatch"));
    }
    if receipt.target_incarnation_ref != envelope.metadata.target_incarnation_ref {
        return Err(invalid("postcondition_receipt_target_mismatch"));
    }
    if receipt.expected_postcondition_contract_refs
        != envelope.metadata.expected_postcondition_contract_refs
    {
        return Err(invalid("postcondition_receipt_expected_contracts_mismatch"));
    }
    if receipt.observation_snapshot_cut_ref.trim().is_empty()
        || receipt.observation_snapshot_cut_ref == envelope.metadata.precondition_snapshot_cut_ref
    {
        return Err(invalid("postcondition_receipt_observation_cut_not_fresh"));
    }
    if receipt.reconciliation_receipt_ref.trim().is_empty() {
        return Err(invalid("postcondition_receipt_missing_reconciliation_ref"));
    }

    let expected_sequence = entries.len() as u64 + 1;
    if receipt.completion_journal_sequence != expected_sequence {
        return Err(invalid("postcondition_receipt_completion_sequence_mismatch"));
    }
    if receipt.receipt_ref != format!("postcondition:{action_id}:{expected_sequence}") {
        return Err(invalid("postcondition_receipt_identity_mismatch"));
    }
    if receipt.causal_assurance.causal_journal_sequence() >= expected_sequence {
        return Err(invalid("postcondition_receipt_causal_sequence_not_prior"));
    }
    if postcondition_observation_cause_for(entries, action_id).as_ref()
        != Some(&receipt.causal_assurance)
    {
        return Err(invalid("postcondition_receipt_causal_history_mismatch"));
    }

    let expected = receipt
        .expected_postcondition_contract_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected.len() != receipt.expected_postcondition_contract_refs.len() || expected.is_empty() {
        return Err(invalid("postcondition_receipt_invalid_expected_contract_set"));
    }

    let mut classified = BTreeSet::new();
    for contract_ref in receipt
        .verified_contract_refs
        .iter()
        .chain(receipt.failed_contract_refs.iter())
        .chain(receipt.unresolved_unknown_contract_refs.iter())
    {
        if !expected.contains(contract_ref) || !classified.insert(contract_ref.clone()) {
            return Err(invalid("postcondition_receipt_invalid_contract_partition"));
        }
    }
    if classified != expected {
        return Err(invalid("postcondition_receipt_incomplete_contract_partition"));
    }

    let expected_verdict = if !receipt.failed_contract_refs.is_empty() {
        ActionPostconditionVerdict::VerifiedUnexpected
    } else if receipt.unresolved_unknown_contract_refs.is_empty() {
        ActionPostconditionVerdict::VerifiedExpected
    } else {
        ActionPostconditionVerdict::ReconciliationRequired
    };
    if receipt.verdict != expected_verdict {
        return Err(invalid("postcondition_receipt_verdict_partition_mismatch"));
    }

    let mut evidence_refs = BTreeSet::new();
    for evidence_ref in &receipt.evidence_receipt_refs {
        if evidence_ref.trim().is_empty() || !evidence_refs.insert(evidence_ref.clone()) {
            return Err(invalid("postcondition_receipt_invalid_evidence_refs"));
        }
    }
    if receipt.evidence_receipt_refs.len() > expected.len()
        || receipt.evidence_receipt_refs.len()
            < receipt.verified_contract_refs.len() + receipt.failed_contract_refs.len()
    {
        return Err(invalid("postcondition_receipt_evidence_cardinality_mismatch"));
    }

    Ok(())
}

'''

journal = replace_once(
    journal,
    '''fn recovery_state_for(\n    entries: &[ConsequentialJournalEntry],\n''',
    validation_helper + '''fn recovery_state_for(\n    entries: &[ConsequentialJournalEntry],\n''',
    "receipt validation helper",
)

journal = replace_once(
    journal,
    '''            ConsequentialJournalTransition::ReconciliationOutcome {\n                world_outcome,\n                postconditions_verified,\n                ..\n            } => {\n                if *postconditions_verified && *world_outcome == WorldOutcome::VerifiedExpected {\n                    ConsequentialRecoveryState::VerifiedUncommitted\n                } else {\n                    ConsequentialRecoveryState::OutcomeObservedUnverified\n                }\n            }\n''',
    '''            ConsequentialJournalTransition::PostconditionReceiptRecorded { receipt } => {\n                if receipt.verdict == ActionPostconditionVerdict::VerifiedExpected {\n                    ConsequentialRecoveryState::VerifiedUncommitted\n                } else {\n                    ConsequentialRecoveryState::OutcomeObservedUnverified\n                }\n            }\n            ConsequentialJournalTransition::ReconciliationOutcome {\n                world_outcome,\n                postconditions_verified,\n                ..\n            } => {\n                if *postconditions_verified && *world_outcome == WorldOutcome::VerifiedExpected {\n                    ConsequentialRecoveryState::VerifiedUncommitted\n                } else {\n                    ConsequentialRecoveryState::OutcomeObservedUnverified\n                }\n            }\n''',
    "recovery mapping",
)

reconcile = replace_once(
    reconcile,
    '''use crate::{\n    CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalEntry,\n    ConsequentialJournalError, ConsequentialJournalTransition,\n    ConsequentialPostconditionObservationReceipt, LiveBridge,\n};\n''',
    '''use crate::{\n    ActionPostconditionReceipt, ActionPostconditionReceiptDraft, ActionPostconditionVerdict,\n    CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalEntry,\n    ConsequentialJournalError, ConsequentialJournalTransition,\n    ConsequentialPostconditionObservationReceipt, LiveBridge,\n};\n''',
    "reconcile imports",
)

reconcile = replace_once(
    reconcile,
    '''pub struct ConsequentialPostconditionReconciliationResult {\n    pub world_outcome: WorldOutcome,\n    pub postconditions_verified: bool,\n    pub journal_entry: ConsequentialJournalEntry,\n}\n''',
    '''pub struct ConsequentialPostconditionReconciliationResult {\n    pub world_outcome: WorldOutcome,\n    pub postconditions_verified: bool,\n    pub journal_entry: ConsequentialJournalEntry,\n    pub postcondition_receipt: ActionPostconditionReceipt,\n}\n''',
    "reconcile result",
)

old_logic = '''    let any_failed = expected.iter().any(|contract_ref| {\n        observed.get(contract_ref).is_some_and(|evidence| {\n            evidence.status == ConsequentialPostconditionStatus::VerifiedFail\n        })\n    });\n    let all_passed = expected.iter().all(|contract_ref| {\n        observed.get(contract_ref).is_some_and(|evidence| {\n            evidence.status == ConsequentialPostconditionStatus::VerifiedPass\n        })\n    });\n\n    let (world_outcome, postconditions_verified) = if any_failed {\n        (WorldOutcome::VerifiedUnexpected, false)\n    } else if all_passed {\n        (WorldOutcome::VerifiedExpected, true)\n    } else {\n        (WorldOutcome::ReconciliationRequired, false)\n    };\n\n    let postcondition_receipt_refs = observed\n        .values()\n        .map(|evidence| evidence.receipt_ref.clone())\n        .collect::<Vec<_>>();\n    let journal_entry = journal\n        .record_reconciliation_outcome(\n            action_id,\n            world_outcome,\n            Some(observation.reconciliation_receipt_ref().to_owned()),\n            postcondition_receipt_refs,\n            postconditions_verified,\n        )\n        .await?;\n\n    Ok(ConsequentialPostconditionReconciliationResult {\n        world_outcome,\n        postconditions_verified,\n        journal_entry,\n    })\n'''
new_logic = '''    let expected_order = envelope.metadata.expected_postcondition_contract_refs.clone();\n    let verified_contract_refs = expected_order\n        .iter()\n        .filter(|contract_ref| {\n            observed.get(*contract_ref).is_some_and(|evidence| {\n                evidence.status == ConsequentialPostconditionStatus::VerifiedPass\n            })\n        })\n        .cloned()\n        .collect::<Vec<_>>();\n    let failed_contract_refs = expected_order\n        .iter()\n        .filter(|contract_ref| {\n            observed.get(*contract_ref).is_some_and(|evidence| {\n                evidence.status == ConsequentialPostconditionStatus::VerifiedFail\n            })\n        })\n        .cloned()\n        .collect::<Vec<_>>();\n    let unresolved_unknown_contract_refs = expected_order\n        .iter()\n        .filter(|contract_ref| {\n            observed\n                .get(*contract_ref)\n                .is_none_or(|evidence| evidence.status == ConsequentialPostconditionStatus::Unknown)\n        })\n        .cloned()\n        .collect::<Vec<_>>();\n    let evidence_receipt_refs = expected_order\n        .iter()\n        .filter_map(|contract_ref| observed.get(contract_ref))\n        .map(|evidence| evidence.receipt_ref.clone())\n        .collect::<Vec<_>>();\n\n    let verdict = if !failed_contract_refs.is_empty() {\n        ActionPostconditionVerdict::VerifiedUnexpected\n    } else if unresolved_unknown_contract_refs.is_empty() {\n        ActionPostconditionVerdict::VerifiedExpected\n    } else {\n        ActionPostconditionVerdict::ReconciliationRequired\n    };\n    let world_outcome = verdict.world_outcome();\n    let postconditions_verified = verdict.postconditions_verified();\n\n    let (journal_entry, postcondition_receipt) = journal\n        .record_action_postcondition_receipt(ActionPostconditionReceiptDraft {\n            action_id,\n            session_id: observation.session_id(),\n            provider_incarnation_ref: observation.provider_incarnation_ref().clone(),\n            target_incarnation_ref: observation.target_incarnation_ref().clone(),\n            expected_postcondition_contract_refs: expected_order,\n            observation_snapshot_cut_ref: observation.snapshot_cut_ref().to_owned(),\n            reconciliation_receipt_ref: observation.reconciliation_receipt_ref().to_owned(),\n            evidence_receipt_refs,\n            verified_contract_refs,\n            failed_contract_refs,\n            verdict,\n            unresolved_unknown_contract_refs,\n            causal_assurance: observation.cause().clone(),\n        })\n        .await?;\n\n    Ok(ConsequentialPostconditionReconciliationResult {\n        world_outcome,\n        postconditions_verified,\n        journal_entry,\n        postcondition_receipt,\n    })\n'''
reconcile = replace_once(reconcile, old_logic, new_logic, "reconcile durable logic")

journal_path.write_text(journal)
reconcile_path.write_text(reconcile)

commands = [
    ["cargo", "fmt", "--all"],
    ["cargo", "check", "-p", "localview-live-bridge", "--all-targets"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_action_postcondition_receipt", "--", "--nocapture"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation_recovery"],
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_consequential_journal"],
    ["cargo", "check", "-p", "localview-windows-observe-runtime", "--all-targets"],
    ["cargo", "test", "-p", "localview-windows-observe-runtime", "--test", "execution_coordinator_behavior"],
    ["git", "diff", "--check"],
]
for command in commands:
    subprocess.run(command, cwd=root, check=True)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], cwd=root, check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], cwd=root, check=True)
subprocess.run([
    "git", "add",
    "crates/live-bridge/src/consequential_journal.rs",
    "crates/live-bridge/src/postcondition_reconciliation.rs",
    "crates/live-bridge/tests/v43_action_postcondition_receipt.rs",
], cwd=root, check=True)
for temp in [
    ".github/scripts/v43_action_postcondition_receipt_green.py",
    ".github/workflows/v43-action-postcondition-receipt-green.yml",
    ".github/workflows/v43-action-postcondition-receipt-red.yml",
]:
    p = root / temp
    if p.exists():
        subprocess.run(["git", "rm", "-f", temp], cwd=root, check=True)
subprocess.run(["git", "commit", "-m", "feat(v43): persist action postcondition receipts"], cwd=root, check=True)
subprocess.run(["git", "push", "origin", "HEAD:feat/v43-durable-action-postcondition-receipt"], cwd=root, check=True)
