use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use localview_live_bridge::{
    ConsequentialJournal, ConsequentialRecoveryInventoryEntry,
};

const CONSEQUENTIAL_JOURNAL_FILE: &str = "consequential-actions.v1.jsonl";

/// Durable recovery authority opened before the daemon advertises readiness.
///
/// Reopening the journal reconstructs durable history only. `ConsequentialJournal`
/// deliberately does not reconstruct any process-local dispatch capabilities,
/// execution permits, or observation grants.
pub(crate) struct BootConsequentialRecovery {
    journal: Arc<ConsequentialJournal>,
    journal_path: PathBuf,
    inventory: Vec<ConsequentialRecoveryInventoryEntry>,
}

impl BootConsequentialRecovery {
    pub(crate) fn journal(&self) -> &Arc<ConsequentialJournal> {
        &self.journal
    }

    pub(crate) fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub(crate) fn inventory(&self) -> &[ConsequentialRecoveryInventoryEntry] {
        &self.inventory
    }
}

pub(crate) async fn open_boot_consequential_recovery(
    state_root: &Path,
) -> Result<BootConsequentialRecovery> {
    tokio::fs::create_dir_all(state_root)
        .await
        .with_context(|| format!("create LocalView state directory {}", state_root.display()))?;
    let journal_path = state_root.join(CONSEQUENTIAL_JOURNAL_FILE);
    let journal = Arc::new(
        ConsequentialJournal::open(&journal_path)
            .await
            .with_context(|| {
                format!(
                    "open durable consequential recovery journal {}",
                    journal_path.display()
                )
            })?,
    );
    let inventory = journal.recovery_inventory().await;

    Ok(BootConsequentialRecovery {
        journal,
        journal_path,
        inventory,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use localview_live_bridge::{
        ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
        ConsequentialJournal, ConsequentialRecoveryState,
    };
    use localview_protocol::{
        PrincipalRef, ProviderIncarnationRef, SessionId, TargetIncarnationRef,
    };
    use uuid::Uuid;

    fn state_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "localview-v43-daemon-recovery-{}",
            Uuid::new_v4()
        ))
    }

    fn envelope() -> CanonicalActionEnvelope {
        CanonicalActionEnvelope {
            envelope_id: Uuid::new_v4(),
            transport_action_id: Uuid::new_v4(),
            session_id: SessionId::new_v4(),
            metadata: ActionEnvelopeMetadata {
                decision_principal_ref: PrincipalRef::from("principal:daemon-test:planner"),
                acting_principal_ref: PrincipalRef::from("principal:daemon-test:executor"),
                authorization_revision: "auth:daemon-test:v1".into(),
                precondition_snapshot_cut_ref: "cut:daemon-test:before".into(),
                provider_incarnation_ref: ProviderIncarnationRef::from("provider:daemon-test:1"),
                target_incarnation_ref: TargetIncarnationRef::from("target:daemon-test:1"),
                risk_class: ActionRiskClass::ExternalSideEffect,
                idempotency_class: ActionIdempotencyClass::Irreversible,
                expected_postcondition_contract_refs: vec![
                    "postcondition:daemon-test:visible".into(),
                ],
            },
        }
    }

    #[tokio::test]
    async fn boot_reopens_durable_journal_and_surfaces_recovery_inventory() {
        let root = state_root();
        std::fs::create_dir_all(&root).unwrap();
        let action = envelope();
        let path = root.join(super::CONSEQUENTIAL_JOURNAL_FILE);
        let journal = ConsequentialJournal::open(&path).await.unwrap();
        let admitted = journal.record_intent_admitted(action.clone()).await.unwrap();
        drop(journal);

        let boot = super::open_boot_consequential_recovery(&root).await.unwrap();
        assert_eq!(boot.journal_path(), path.as_path());
        assert_eq!(boot.inventory().len(), 1);
        assert_eq!(boot.inventory()[0].action_id, action.transport_action_id);
        assert_eq!(
            boot.inventory()[0].recovery_state,
            ConsequentialRecoveryState::Admitted
        );
        assert_eq!(
            boot.inventory()[0].latest_journal_sequence,
            admitted.journal_sequence
        );
        assert_eq!(
            boot.journal()
                .recovery_state(action.transport_action_id)
                .await,
            Some(ConsequentialRecoveryState::Admitted)
        );

        drop(boot);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn boot_creates_state_directory_before_opening_journal() {
        let root = state_root();
        assert!(!root.exists());

        let boot = super::open_boot_consequential_recovery(&root).await.unwrap();
        assert!(root.is_dir());
        assert!(boot.journal_path().exists());
        assert!(boot.inventory().is_empty());

        drop(boot);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn attachment_recovery_dispositions_are_fail_closed() {
        use super::{AttachmentRecoveryDisposition, classify_attachment_recovery_state};

        let cases = [
            (
                ConsequentialRecoveryState::VerifiedUncommitted,
                AttachmentRecoveryDisposition::CommitOnly,
            ),
            (
                ConsequentialRecoveryState::Committed,
                AttachmentRecoveryDisposition::HistoricalCommitted,
            ),
            (
                ConsequentialRecoveryState::DispatchPrepared,
                AttachmentRecoveryDisposition::VerifierRequired,
            ),
            (
                ConsequentialRecoveryState::PossiblyDispatched,
                AttachmentRecoveryDisposition::VerifierRequired,
            ),
            (
                ConsequentialRecoveryState::OutcomeObservedUnverified,
                AttachmentRecoveryDisposition::VerifierRequired,
            ),
            (
                ConsequentialRecoveryState::Admitted,
                AttachmentRecoveryDisposition::NoProviderRecovery,
            ),
            (
                ConsequentialRecoveryState::AuthorizedNotDispatched,
                AttachmentRecoveryDisposition::NoProviderRecovery,
            ),
            (
                ConsequentialRecoveryState::KnownNotDispatched,
                AttachmentRecoveryDisposition::NoProviderRecovery,
            ),
            (
                ConsequentialRecoveryState::Compensated,
                AttachmentRecoveryDisposition::NoProviderRecovery,
            ),
            (
                ConsequentialRecoveryState::CompensationFailed,
                AttachmentRecoveryDisposition::NoProviderRecovery,
            ),
        ];

        for (state, expected) in cases {
            assert_eq!(classify_attachment_recovery_state(state), expected);
        }
    }

    #[tokio::test]
    async fn attachment_plan_contains_only_exact_durable_lineage() {
        let root = state_root();
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(super::CONSEQUENTIAL_JOURNAL_FILE);
        let journal = ConsequentialJournal::open(&path).await.unwrap();

        let exact = envelope();
        let mut wrong_provider = exact.clone();
        wrong_provider.envelope_id = Uuid::new_v4();
        wrong_provider.transport_action_id = Uuid::new_v4();
        wrong_provider.metadata.provider_incarnation_ref =
            ProviderIncarnationRef::from("provider:daemon-test:wrong");

        journal.record_intent_admitted(exact.clone()).await.unwrap();
        journal.record_intent_admitted(wrong_provider).await.unwrap();

        let plan = super::plan_attachment_recovery(
            &journal,
            exact.session_id,
            &exact.metadata.provider_incarnation_ref,
            &exact.metadata.target_incarnation_ref,
        )
        .await;

        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].action_id, exact.transport_action_id);
        assert_eq!(
            plan[0].disposition,
            super::AttachmentRecoveryDisposition::NoProviderRecovery
        );
        assert_eq!(
            plan[0].expected_postcondition_contract_refs,
            exact.metadata.expected_postcondition_contract_refs
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
