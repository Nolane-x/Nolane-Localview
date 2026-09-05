from pathlib import Path
import subprocess

BRANCH = "feat/v43-consequential-postcondition-reconciliation-clean"

root = Path.cwd()
v43 = root / "crates/live-bridge/src/v43_lib.rs"
journal = root / "crates/live-bridge/src/consequential_journal.rs"
module = root / "crates/live-bridge/src/postcondition_reconciliation.rs"

v43_text = v43.read_text()
if "mod postcondition_reconciliation;" not in v43_text:
    v43_text = v43_text.replace(
        "mod consequential_journal;\n",
        "mod consequential_journal;\nmod postcondition_reconciliation;\n",
        1,
    )
if "pub use postcondition_reconciliation::*;" not in v43_text:
    v43_text = v43_text.replace(
        "pub use consequential_journal::*;\n",
        "pub use consequential_journal::*;\npub use postcondition_reconciliation::*;\n",
        1,
    )
getter = '''    pub(crate) async fn current_reconciliation_snapshot(
        &self,
        session_id: SessionId,
    ) -> Option<ReconciliationSnapshotReceipt> {
        self.continuity
            .read()
            .await
            .get(&session_id)
            .and_then(|state| state.reconciliation.clone())
    }

'''
if "current_reconciliation_snapshot" not in v43_text:
    marker = "    pub async fn record_reconciliation(\n"
    if marker not in v43_text:
        raise SystemExit("record_reconciliation sentinel missing")
    v43_text = v43_text.replace(marker, getter + marker, 1)
v43.write_text(v43_text)

journal_text = journal.read_text()
old = '''            Some(ConsequentialRecoveryState::DispatchPrepared)
            | Some(ConsequentialRecoveryState::PossiblyDispatched)
            | Some(ConsequentialRecoveryState::KnownNotDispatched) => Ok(()),
'''
new = '''            Some(ConsequentialRecoveryState::DispatchPrepared)
            | Some(ConsequentialRecoveryState::PossiblyDispatched)
            | Some(ConsequentialRecoveryState::KnownNotDispatched)
            | Some(ConsequentialRecoveryState::OutcomeObservedUnverified) => Ok(()),
'''
if old not in journal_text and new not in journal_text:
    raise SystemExit("reconciliation transition sentinel missing")
if new not in journal_text:
    journal_text = journal_text.replace(old, new, 1)
journal.write_text(journal_text)

module.write_text(r'''use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::{
    ProviderIncarnationRef, ReconciliationCompleteness, TargetIncarnationRef, WorldOutcome,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalEntry,
    ConsequentialJournalError, ConsequentialJournalTransition, LiveBridge,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConsequentialPostconditionStatus {
    VerifiedPass,
    VerifiedFail,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsequentialPostconditionEvidence {
    pub contract_ref: String,
    pub status: ConsequentialPostconditionStatus,
    pub receipt_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsequentialPostconditionReconciliationReceipt {
    pub action_id: Uuid,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub snapshot_cut_ref: String,
    pub reconciliation_receipt_ref: String,
    pub postconditions: Vec<ConsequentialPostconditionEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialPostconditionReconciliationResult {
    pub world_outcome: WorldOutcome,
    pub postconditions_verified: bool,
    pub journal_entry: ConsequentialJournalEntry,
}

#[derive(Debug, Error)]
pub enum ConsequentialReconciliationError {
    #[error("unknown consequential action {action_id}")]
    UnknownAction { action_id: Uuid },
    #[error("consequential action provider incarnation does not match reconciliation evidence")]
    ProviderIncarnationMismatch,
    #[error("consequential action target incarnation does not match reconciliation evidence")]
    TargetIncarnationMismatch,
    #[error("reconciliation snapshot is not the exact current evidence bound to this action")]
    ReconciliationSnapshotMismatch,
    #[error("reconciliation snapshot is incomplete or carries incompleteness debt")]
    ReconciliationSnapshotIncomplete,
    #[error("consequential action has no declared expected postconditions")]
    MissingExpectedPostconditions,
    #[error("consequential action declares duplicate expected postcondition {contract_ref}")]
    DuplicateExpectedPostcondition { contract_ref: String },
    #[error("postcondition evidence contains duplicate contract {contract_ref}")]
    DuplicatePostconditionEvidence { contract_ref: String },
    #[error("postcondition evidence contains undeclared contract {contract_ref}")]
    UnexpectedPostconditionEvidence { contract_ref: String },
    #[error("postcondition evidence for {contract_ref} has no durable receipt reference")]
    MissingPostconditionReceipt { contract_ref: String },
    #[error(transparent)]
    Journal(#[from] ConsequentialJournalError),
}

pub async fn reconcile_consequential_postconditions(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    receipt: ConsequentialPostconditionReconciliationReceipt,
) -> Result<ConsequentialPostconditionReconciliationResult, ConsequentialReconciliationError> {
    let envelope = admitted_envelope(journal, receipt.action_id)
        .await
        .ok_or(ConsequentialReconciliationError::UnknownAction {
            action_id: receipt.action_id,
        })?;

    if receipt.provider_incarnation_ref != envelope.metadata.provider_incarnation_ref {
        return Err(ConsequentialReconciliationError::ProviderIncarnationMismatch);
    }
    if receipt.target_incarnation_ref != envelope.metadata.target_incarnation_ref {
        return Err(ConsequentialReconciliationError::TargetIncarnationMismatch);
    }

    let snapshot = bridge
        .current_reconciliation_snapshot(envelope.session_id)
        .await
        .ok_or(ConsequentialReconciliationError::ReconciliationSnapshotMismatch)?;
    if snapshot.receipt_id != receipt.reconciliation_receipt_ref
        || snapshot.snapshot_cut_ref != receipt.snapshot_cut_ref
        || snapshot.provider_incarnation_ref != receipt.provider_incarnation_ref
        || snapshot.target_incarnation_ref != receipt.target_incarnation_ref
    {
        return Err(ConsequentialReconciliationError::ReconciliationSnapshotMismatch);
    }
    if snapshot.completeness != ReconciliationCompleteness::Established
        || !snapshot.incompleteness_debt.is_empty()
    {
        return Err(ConsequentialReconciliationError::ReconciliationSnapshotIncomplete);
    }

    let expected = exact_expected_postconditions(&envelope)?;
    let observed = exact_observed_postconditions(&expected, receipt.postconditions)?;

    let any_failed = expected.iter().any(|contract_ref| {
        observed
            .get(contract_ref)
            .is_some_and(|evidence| evidence.status == ConsequentialPostconditionStatus::VerifiedFail)
    });
    let all_passed = expected.iter().all(|contract_ref| {
        observed
            .get(contract_ref)
            .is_some_and(|evidence| evidence.status == ConsequentialPostconditionStatus::VerifiedPass)
    });

    let (world_outcome, postconditions_verified) = if any_failed {
        (WorldOutcome::VerifiedUnexpected, false)
    } else if all_passed {
        (WorldOutcome::VerifiedExpected, true)
    } else {
        (WorldOutcome::ReconciliationRequired, false)
    };

    let postcondition_receipt_refs = observed
        .values()
        .map(|evidence| evidence.receipt_ref.clone())
        .collect::<Vec<_>>();
    let journal_entry = journal
        .record_reconciliation_outcome(
            receipt.action_id,
            world_outcome,
            Some(receipt.reconciliation_receipt_ref),
            postcondition_receipt_refs,
            postconditions_verified,
        )
        .await?;

    Ok(ConsequentialPostconditionReconciliationResult {
        world_outcome,
        postconditions_verified,
        journal_entry,
    })
}

async fn admitted_envelope(
    journal: &ConsequentialJournal,
    action_id: Uuid,
) -> Option<CanonicalActionEnvelope> {
    journal
        .entries_for(action_id)
        .await
        .into_iter()
        .find_map(|entry| match entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
            _ => None,
        })
}

fn exact_expected_postconditions(
    envelope: &CanonicalActionEnvelope,
) -> Result<BTreeSet<String>, ConsequentialReconciliationError> {
    if envelope.metadata.expected_postcondition_contract_refs.is_empty() {
        return Err(ConsequentialReconciliationError::MissingExpectedPostconditions);
    }

    let mut expected = BTreeSet::new();
    for contract_ref in &envelope.metadata.expected_postcondition_contract_refs {
        if !expected.insert(contract_ref.clone()) {
            return Err(
                ConsequentialReconciliationError::DuplicateExpectedPostcondition {
                    contract_ref: contract_ref.clone(),
                },
            );
        }
    }
    Ok(expected)
}

fn exact_observed_postconditions(
    expected: &BTreeSet<String>,
    evidence: Vec<ConsequentialPostconditionEvidence>,
) -> Result<BTreeMap<String, ConsequentialPostconditionEvidence>, ConsequentialReconciliationError> {
    let mut observed = BTreeMap::new();
    for item in evidence {
        if !expected.contains(&item.contract_ref) {
            return Err(
                ConsequentialReconciliationError::UnexpectedPostconditionEvidence {
                    contract_ref: item.contract_ref,
                },
            );
        }
        if item.receipt_ref.trim().is_empty() {
            return Err(ConsequentialReconciliationError::MissingPostconditionReceipt {
                contract_ref: item.contract_ref,
            });
        }
        if observed.contains_key(&item.contract_ref) {
            return Err(
                ConsequentialReconciliationError::DuplicatePostconditionEvidence {
                    contract_ref: item.contract_ref,
                },
            );
        }
        observed.insert(item.contract_ref.clone(), item);
    }
    Ok(observed)
}
''')

subprocess.run(["cargo", "fmt", "--all"], check=True)
subprocess.run(
    ["cargo", "check", "-p", "localview-live-bridge", "--all-targets"], check=True
)
subprocess.run(
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_postcondition_reconciliation", "--", "--nocapture"],
    check=True,
)
subprocess.run(
    ["cargo", "test", "-p", "localview-live-bridge", "--test", "v43_consequential_journal"],
    check=True,
)

subprocess.run(["git", "config", "user.name", "github-actions[bot]"], check=True)
subprocess.run(["git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"], check=True)
subprocess.run(
    [
        "git",
        "add",
        "crates/live-bridge/src/v43_lib.rs",
        "crates/live-bridge/src/consequential_journal.rs",
        "crates/live-bridge/src/postcondition_reconciliation.rs",
    ],
    check=True,
)
subprocess.run(
    ["git", "rm", ".github/workflows/v43-postcondition-builder.yml", ".github/scripts/v43_postcondition_builder.py"],
    check=True,
)
subprocess.run(
    ["git", "commit", "-m", "feat(v43): bind typed postcondition reconciliation to durable evidence"],
    check=True,
)
subprocess.run(["git", "push", "origin", f"HEAD:{BRANCH}"], check=True)
