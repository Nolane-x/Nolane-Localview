use std::collections::{BTreeMap, BTreeSet};

use localview_protocol::{ReconciliationCompleteness, WorldOutcome};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    ActionPostconditionReceipt, ActionPostconditionReceiptDraft, ActionPostconditionVerdict,
    CanonicalActionEnvelope, ConsequentialJournal, ConsequentialJournalEntry,
    ConsequentialJournalError, ConsequentialJournalTransition,
    ConsequentialPostconditionObservationReceipt, LiveBridge,
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

/// Typed reconciliation input whose causal lineage can only originate from a
/// journal-minted post-dispatch observation receipt.
///
/// The observation binding and postcondition list are deliberately private. A
/// caller may supply predicate evidence, but it cannot choose the action,
/// provider/target incarnation, observation cut, or reconciliation receipt that
/// authorizes that evidence to affect consequential world state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialPostconditionReconciliationReceipt {
    observation: ConsequentialPostconditionObservationReceipt,
    postconditions: Vec<ConsequentialPostconditionEvidence>,
}

impl ConsequentialPostconditionReconciliationReceipt {
    pub fn from_observation(
        observation: ConsequentialPostconditionObservationReceipt,
        postconditions: Vec<ConsequentialPostconditionEvidence>,
    ) -> Self {
        Self {
            observation,
            postconditions,
        }
    }

    pub fn observation(&self) -> &ConsequentialPostconditionObservationReceipt {
        &self.observation
    }

    pub fn postconditions(&self) -> &[ConsequentialPostconditionEvidence] {
        &self.postconditions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsequentialPostconditionReconciliationResult {
    pub world_outcome: WorldOutcome,
    pub postconditions_verified: bool,
    pub journal_entry: ConsequentialJournalEntry,
    pub postcondition_receipt: ActionPostconditionReceipt,
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

/// Reconcile one consequential action from independently observed postcondition evidence.
///
/// This is the public authority boundary for postcondition outcome writes. The
/// causal observation lineage is not caller-authored: it comes from the opaque
/// receipt minted by `ConsequentialJournal::complete_postcondition_observation`.
/// The caller supplies only typed predicate evidence for declared contracts and
/// never chooses `postconditions_verified` or a verified world outcome directly.
pub async fn reconcile_consequential_postconditions(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    receipt: ConsequentialPostconditionReconciliationReceipt,
) -> Result<ConsequentialPostconditionReconciliationResult, ConsequentialReconciliationError> {
    let ConsequentialPostconditionReconciliationReceipt {
        observation,
        postconditions,
    } = receipt;
    let action_id = observation.action_id();
    let envelope = admitted_envelope(journal, action_id)
        .await
        .ok_or(ConsequentialReconciliationError::UnknownAction { action_id })?;

    if observation.provider_incarnation_ref() != &envelope.metadata.provider_incarnation_ref {
        return Err(ConsequentialReconciliationError::ProviderIncarnationMismatch);
    }
    if observation.target_incarnation_ref() != &envelope.metadata.target_incarnation_ref {
        return Err(ConsequentialReconciliationError::TargetIncarnationMismatch);
    }
    if observation.session_id() != envelope.session_id {
        return Err(ConsequentialReconciliationError::ReconciliationSnapshotMismatch);
    }

    let snapshot = bridge
        .current_reconciliation_snapshot(envelope.session_id)
        .await
        .ok_or(ConsequentialReconciliationError::ReconciliationSnapshotMismatch)?;
    if snapshot.receipt_id != observation.reconciliation_receipt_ref()
        || snapshot.snapshot_cut_ref != observation.snapshot_cut_ref()
        || &snapshot.provider_incarnation_ref != observation.provider_incarnation_ref()
        || &snapshot.target_incarnation_ref != observation.target_incarnation_ref()
    {
        return Err(ConsequentialReconciliationError::ReconciliationSnapshotMismatch);
    }
    if snapshot.completeness != ReconciliationCompleteness::Established
        || !snapshot.incompleteness_debt.is_empty()
    {
        return Err(ConsequentialReconciliationError::ReconciliationSnapshotIncomplete);
    }

    let expected = exact_expected_postconditions(&envelope)?;
    let observed = exact_observed_postconditions(&expected, postconditions)?;

    let expected_order = envelope
        .metadata
        .expected_postcondition_contract_refs
        .clone();
    let verified_contract_refs = expected_order
        .iter()
        .filter(|contract_ref| {
            observed.get(*contract_ref).is_some_and(|evidence| {
                evidence.status == ConsequentialPostconditionStatus::VerifiedPass
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let failed_contract_refs = expected_order
        .iter()
        .filter(|contract_ref| {
            observed.get(*contract_ref).is_some_and(|evidence| {
                evidence.status == ConsequentialPostconditionStatus::VerifiedFail
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let unresolved_unknown_contract_refs = expected_order
        .iter()
        .filter(|contract_ref| {
            observed
                .get(*contract_ref)
                .is_none_or(|evidence| evidence.status == ConsequentialPostconditionStatus::Unknown)
        })
        .cloned()
        .collect::<Vec<_>>();
    let evidence_receipt_refs = expected_order
        .iter()
        .filter_map(|contract_ref| observed.get(contract_ref))
        .map(|evidence| evidence.receipt_ref.clone())
        .collect::<Vec<_>>();

    let verdict = if !failed_contract_refs.is_empty() {
        ActionPostconditionVerdict::VerifiedUnexpected
    } else if unresolved_unknown_contract_refs.is_empty() {
        ActionPostconditionVerdict::VerifiedExpected
    } else {
        ActionPostconditionVerdict::ReconciliationRequired
    };
    let world_outcome = verdict.world_outcome();
    let postconditions_verified = verdict.postconditions_verified();

    let (journal_entry, postcondition_receipt) = journal
        .record_action_postcondition_receipt(ActionPostconditionReceiptDraft {
            action_id,
            session_id: observation.session_id(),
            provider_incarnation_ref: observation.provider_incarnation_ref().clone(),
            target_incarnation_ref: observation.target_incarnation_ref().clone(),
            expected_postcondition_contract_refs: expected_order,
            observation_snapshot_cut_ref: observation.snapshot_cut_ref().to_owned(),
            reconciliation_receipt_ref: observation.reconciliation_receipt_ref().to_owned(),
            evidence_receipt_refs,
            verified_contract_refs,
            failed_contract_refs,
            verdict,
            unresolved_unknown_contract_refs,
            causal_assurance: observation.cause().clone(),
        })
        .await?;

    Ok(ConsequentialPostconditionReconciliationResult {
        world_outcome,
        postconditions_verified,
        journal_entry,
        postcondition_receipt,
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
    if envelope
        .metadata
        .expected_postcondition_contract_refs
        .is_empty()
    {
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
) -> Result<BTreeMap<String, ConsequentialPostconditionEvidence>, ConsequentialReconciliationError>
{
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
            return Err(
                ConsequentialReconciliationError::MissingPostconditionReceipt {
                    contract_ref: item.contract_ref,
                },
            );
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
