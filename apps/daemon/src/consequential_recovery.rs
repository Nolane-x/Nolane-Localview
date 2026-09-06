use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use localview_live_bridge::{
    ConsequentialJournal, ConsequentialRecoveryInventoryEntry, LiveBridge,
};
use localview_protocol::{ProviderIncarnationRef, SessionId, TargetIncarnationRef};
use localview_windows_observe_runtime::{
    recover_attached_consequential_debt, WindowsObserveProvider, WindowsObserveRuntimeManager,
    WindowsUiaAttachedRecoveryDrain, WindowsUiaAttachedRecoveryDrainError,
    WindowsUiaPostconditionVerifier, WindowsUiaSemanticPostconditionVerifier,
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

/// Compatibility name retained for the daemon recovery loop.
///
/// The implementation is now the production typed semantic verifier. Supported
/// V1 contracts can produce evidence from the exact immutable recovery snapshot;
/// unknown, legacy, malformed, or unsupported contract refs remain `Unknown`, so
/// this surface stays fail-closed without blocking typed postconditions forever.
pub(crate) type FailClosedWindowsPostconditionVerifier = WindowsUiaSemanticPostconditionVerifier;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsRecoveryAttachmentLineage {
    session_id: SessionId,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
}

/// Process-local guard preventing unresolved boot debt from being observed and
/// reconciled forever on a stable attachment.
///
/// This tracker is intentionally non-durable and contains no authority. A new
/// daemon process may re-observe unresolved debt after the user re-establishes an
/// exact attachment, while one process attempts each exact attachment lineage at
/// most once after a successful recovery drain.
#[derive(Debug, Default)]
pub(crate) struct WindowsBootRecoveryTracker {
    completed_lineages: Vec<WindowsRecoveryAttachmentLineage>,
}

impl WindowsBootRecoveryTracker {
    fn contains(&self, lineage: &WindowsRecoveryAttachmentLineage) -> bool {
        self.completed_lineages.contains(lineage)
    }

    fn record_completed(&mut self, drain: &WindowsUiaAttachedRecoveryDrain) {
        let lineage = WindowsRecoveryAttachmentLineage {
            session_id: drain.session_id,
            provider_incarnation_ref: drain.provider_incarnation_ref.clone(),
            target_incarnation_ref: drain.target_incarnation_ref.clone(),
        };
        if !self.contains(&lineage) {
            self.completed_lineages.push(lineage);
        }
    }
}

/// Recover durable boot history only after an exact Windows UIA attachment has
/// been established by normal control-plane authority.
///
/// The function accepts no executor, dispatch permit, or execution grant. It
/// delegates only to the attachment-bound no-redispatch recovery coordinator.
/// Failed drains are deliberately not tracked so transient provider/resource
/// failures remain retryable; successful drains, including unresolved outcomes,
/// are attempted only once per exact attachment lineage in this daemon process.
pub(crate) async fn recover_newly_attached_boot_debt<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    verifier: &V,
    tracker: &mut WindowsBootRecoveryTracker,
) -> Result<Vec<WindowsUiaAttachedRecoveryDrain>, WindowsUiaAttachedRecoveryDrainError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let mut drains = Vec::new();

    for session_id in runtime.attached_sessions().await {
        let Some(snapshot) = runtime.current_semantic_snapshot(session_id).await else {
            continue;
        };
        let candidate = WindowsRecoveryAttachmentLineage {
            session_id,
            provider_incarnation_ref: snapshot.provider_incarnation_ref().clone(),
            target_incarnation_ref: snapshot.target_incarnation_ref().clone(),
        };
        if tracker.contains(&candidate) {
            continue;
        }

        let drain = recover_attached_consequential_debt(
            bridge,
            journal,
            runtime,
            session_id,
            verifier,
        )
        .await?;
        tracker.record_completed(&drain);
        drains.push(drain);
    }

    Ok(drains)
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
}

#[cfg(test)]
#[path = "consequential_recovery_runtime_tests.rs"]
mod runtime_tests;
