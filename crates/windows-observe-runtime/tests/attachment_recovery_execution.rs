use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialPostconditionEvidence,
    ConsequentialPostconditionReconciliationReceipt, ConsequentialPostconditionStatus,
    ConsequentialRecoveryState, DispatchPreparationReceipt, LiveBridge,
    reconcile_consequential_postconditions,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    PrincipalRef, ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, SessionId, TargetIncarnationRef, WorldOutcome,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaAttachedRecoveryDrainOutcome,
    WindowsUiaConsequentialRecoveryOutcome, WindowsUiaPostconditionVerifier,
    recover_attached_consequential_debt,
};
use localview_windows_uia_provider::WindowsUiaEventDrain;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake provider error")]
struct FakeProviderError;

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    snapshot_calls: Arc<Mutex<usize>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:recovery-execution"),
            target: TargetIncarnationRef::from("target:windows:recovery-execution"),
            snapshot_calls: Arc::new(Mutex::new(0)),
        }
    }

    fn snapshot_calls(&self) -> usize {
        *self.snapshot_calls.lock().unwrap()
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[recovery-execution]".into(),
                semantic_locator_hints: vec![],
                parent_surface_ref: Some("window:recovery-execution".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("window".into()),
            name: Some("Recovery Execution".into()),
            control_type: Some("uia_control_type:50032".into()),
            automation_id: None,
            class_name: Some("Window".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes: BTreeMap::new(),
        };
        let mut cache =
            SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:recovery-execution".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![node],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 1,
                    properties_read: 1,
                    max_depth_observed: 0,
                    exhausted: vec![],
                    incomplete: false,
                },
                completeness: ReconciliationCompleteness::Established,
                incompleteness_debt: vec![],
            })
            .unwrap()
    }
}

impl WindowsObserveProvider for FakeProvider {
    type Attachment = FakeAttachment;
    type Subscription = FakeSubscription;
    type Error = FakeProviderError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.provider.clone()
    }

    fn attach(&self, _selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error> {
        Ok(FakeAttachment(self.target.clone()))
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.0.clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        _capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        Ok(FakeSubscription(WindowsObserveSubscriptionLineage {
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: attachment.0.clone(),
            sequence_baseline: 0,
        }))
    }

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage {
        subscription.0.clone()
    }

    fn drain_events(
        &self,
        _subscription: &Self::Subscription,
        _limit: usize,
    ) -> Result<WindowsUiaEventDrain, Self::Error> {
        Ok(WindowsUiaEventDrain {
            events: vec![],
            dropped_before_drain: 0,
            latest_sequence: 0,
        })
    }

    fn snapshot(
        &self,
        _attachment: &Self::Attachment,
        snapshot_cut_ref: String,
        _surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        *self.snapshot_calls.lock().unwrap() += 1;
        Ok(self.build_snapshot(snapshot_cut_ref))
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Error)]
#[error("fake verifier error")]
struct FakeVerifierError;

#[derive(Debug)]
struct FakeVerifier {
    calls: Mutex<usize>,
}

impl FakeVerifier {
    fn new() -> Self {
        Self {
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl WindowsUiaPostconditionVerifier for FakeVerifier {
    type Error = FakeVerifierError;

    fn verify(
        &self,
        _action_id: Uuid,
        expected_contract_refs: &[String],
        snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        *self.calls.lock().unwrap() += 1;
        Ok(expected_contract_refs
            .iter()
            .map(|contract_ref| ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status: ConsequentialPostconditionStatus::VerifiedPass,
                receipt_ref: format!("evidence:{}:{contract_ref}", snapshot.snapshot_cut_ref()),
            })
            .collect())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x8301)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x8302,
        expected_process_id: 83,
        selection_nonce: Uuid::from_u128(0x8303),
    }
}

fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-v43-attachment-recovery-execution-{}.jsonl",
        Uuid::new_v4()
    ))
}

fn envelope(
    label: &str,
    provider: &FakeProvider,
) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: session(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from(format!("principal:planner:{label}")),
            acting_principal_ref: PrincipalRef::from(format!("principal:executor:{label}")),
            authorization_revision: format!("authorization:{label}:v1"),
            precondition_snapshot_cut_ref: format!("cut:{label}:before"),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec![format!("postcondition:{label}")],
        },
    }
}

async fn record_authorized_not_dispatched(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) {
    journal.record_intent_admitted(action.clone()).await.unwrap();
    journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
}

async fn record_prepared(
    journal: &ConsequentialJournal,
    action: &CanonicalActionEnvelope,
) {
    journal.record_intent_admitted(action.clone()).await.unwrap();
    let authorization = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let admission = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            DispatchPreparationReceipt {
                receipt_ref: format!("prepared:{}", action.transport_action_id),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: action
                    .metadata
                    .precondition_snapshot_cut_ref
                    .clone(),
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    drop(admission);
}

#[tokio::test]
async fn attachment_recovery_executes_only_typed_recovery_dispositions_without_redispatch_surface() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime.attach(session(), selection()).await.unwrap();
    assert_eq!(provider.snapshot_calls(), 1);

    let path = path();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let no_dispatch = envelope("no-dispatch", &provider);
    record_authorized_not_dispatched(&journal, &no_dispatch).await;
    let uncertain = envelope("uncertain", &provider);
    record_prepared(&journal, &uncertain).await;
    drop(journal);

    // Reopen proves no process-local dispatch capability survives restart.
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let verifier = FakeVerifier::new();
    let drain = recover_attached_consequential_debt(
        &bridge,
        &journal,
        &runtime,
        session(),
        &verifier,
    )
    .await
    .unwrap();

    assert_eq!(drain.entries.len(), 2);
    assert!(matches!(
        &drain.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::NoDispatchProven {
            action_id,
            durable_state: ConsequentialRecoveryState::AuthorizedNotDispatched,
        } if *action_id == no_dispatch.transport_action_id
    ));
    assert!(matches!(
        &drain.entries[1],
        WindowsUiaAttachedRecoveryDrainOutcome::Recovered(
            WindowsUiaConsequentialRecoveryOutcome::ReconciledCommitted {
                action_id,
                world_outcome: WorldOutcome::VerifiedExpected,
                ..
            }
        ) if *action_id == uncertain.transport_action_id
    ));
    assert_eq!(
        journal.recovery_state(uncertain.transport_action_id).await,
        Some(ConsequentialRecoveryState::Committed)
    );
    assert_eq!(provider.snapshot_calls(), 2);
    assert_eq!(verifier.calls(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn commit_only_and_historical_recovery_never_recaptures_or_reverifies() {
    let bridge = LiveBridge::new(64, 8);
    let provider = FakeProvider::new();
    let runtime = WindowsObserveRuntimeManager::new(
        Arc::new(provider.clone()),
        bridge.clone(),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    runtime.attach(session(), selection()).await.unwrap();

    let path = path();
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let action = envelope("commit-only", &provider);
    record_prepared(&journal, &action).await;
    drop(journal);
    let journal = ConsequentialJournal::open(&path).await.unwrap();

    let permit = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let capture = runtime
        .capture_postcondition_observation_with_snapshot(&journal, permit)
        .await
        .unwrap();
    let contract_ref = action.metadata.expected_postcondition_contract_refs[0].clone();
    let evidence = vec![ConsequentialPostconditionEvidence {
        contract_ref: contract_ref.clone(),
        status: ConsequentialPostconditionStatus::VerifiedPass,
        receipt_ref: format!("evidence:commit-only:{contract_ref}"),
    }];
    let reconciliation = reconcile_consequential_postconditions(
        &bridge,
        &journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            capture.into_observation_receipt(),
            evidence,
        ),
    )
    .await
    .unwrap();
    assert_eq!(reconciliation.world_outcome, WorldOutcome::VerifiedExpected);
    assert_eq!(
        journal.recovery_state(action.transport_action_id).await,
        Some(ConsequentialRecoveryState::VerifiedUncommitted)
    );

    let snapshots_before = provider.snapshot_calls();
    let verifier = FakeVerifier::new();
    let commit_only = recover_attached_consequential_debt(
        &bridge,
        &journal,
        &runtime,
        session(),
        &verifier,
    )
    .await
    .unwrap();
    assert!(matches!(
        &commit_only.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::Recovered(
            WindowsUiaConsequentialRecoveryOutcome::CommittedFromDurableReceipt { action_id, .. }
        ) if *action_id == action.transport_action_id
    ));
    assert_eq!(provider.snapshot_calls(), snapshots_before);
    assert_eq!(verifier.calls(), 0);

    let historical = recover_attached_consequential_debt(
        &bridge,
        &journal,
        &runtime,
        session(),
        &verifier,
    )
    .await
    .unwrap();
    assert!(matches!(
        &historical.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::HistoricalTerminal {
            action_id,
            durable_state: ConsequentialRecoveryState::Committed,
        } if *action_id == action.transport_action_id
    ));
    assert_eq!(provider.snapshot_calls(), snapshots_before);
    assert_eq!(verifier.calls(), 0);

    let _ = std::fs::remove_file(path);
}
