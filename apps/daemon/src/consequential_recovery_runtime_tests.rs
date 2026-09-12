use std::{
    collections::BTreeMap,
    error::Error as StdError,
    fmt,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialPostconditionEvidence, ConsequentialPostconditionStatus,
    ConsequentialRecoveryActionScope, ConsequentialRecoveryState, DispatchPreparationReceipt,
    LiveBridge,
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
};
use localview_windows_uia_provider::WindowsUiaEventDrain;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone)]
struct FakeProviderError;

impl fmt::Display for FakeProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("fake provider error")
    }
}

impl StdError for FakeProviderError {}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    snapshot_calls: Arc<Mutex<usize>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:daemon-recovery:1"),
            target: TargetIncarnationRef::from("target:daemon-recovery:1"),
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
                opaque_provider_element_id: "uia-runtime:[daemon-recovery]".into(),
                semantic_locator_hints: vec![],
                parent_surface_ref: Some("window:daemon-recovery".into()),
                acquisition_cut_ref: cut.clone(),
                realization: ProviderElementRealization::RealizedCurrent,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("window".into()),
            name: Some("Daemon Recovery".into()),
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
                surface_scope: "window:daemon-recovery".into(),
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

    fn attach(
        &self,
        _selection: UserSelectedWindowTarget,
    ) -> Result<Self::Attachment, Self::Error> {
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

#[derive(Debug, Clone)]
struct SelectiveVerifierError;

impl fmt::Display for SelectiveVerifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("selective verifier failure")
    }
}

impl StdError for SelectiveVerifierError {}

struct SelectiveVerifier {
    failing_action_id: Uuid,
}

impl WindowsUiaPostconditionVerifier for SelectiveVerifier {
    type Error = SelectiveVerifierError;

    fn verify(
        &self,
        action_id: Uuid,
        expected_contract_refs: &[String],
        _snapshot: &NativeSemanticSnapshotRevision,
    ) -> Result<Vec<ConsequentialPostconditionEvidence>, Self::Error> {
        if action_id == self.failing_action_id {
            return Err(SelectiveVerifierError);
        }
        Ok(expected_contract_refs
            .iter()
            .enumerate()
            .map(|(index, contract_ref)| ConsequentialPostconditionEvidence {
                contract_ref: contract_ref.clone(),
                status: ConsequentialPostconditionStatus::Unknown,
                receipt_ref: format!("selective-verifier:{action_id}:{index}"),
            })
            .collect())
    }
}

fn recovery_session() -> SessionId {
    Uuid::from_u128(0x8401)
}

fn second_recovery_session() -> SessionId {
    Uuid::from_u128(0x8405)
}

fn selection() -> UserSelectedWindowTarget {
    selection_for(0x8402, 0x8403)
}

fn second_selection() -> UserSelectedWindowTarget {
    selection_for(0x8406, 0x8407)
}

fn selection_for(native_window_handle: u64, selection_nonce: u128) -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle,
        expected_process_id: 84,
        selection_nonce: Uuid::from_u128(selection_nonce),
    }
}

fn recovery_envelope(provider: &FakeProvider) -> CanonicalActionEnvelope {
    recovery_envelope_for_session(provider, recovery_session())
}

fn recovery_envelope_for_session(
    provider: &FakeProvider,
    session_id: SessionId,
) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id,
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:daemon-recovery:planner"),
            acting_principal_ref: PrincipalRef::from("principal:daemon-recovery:executor"),
            authorization_revision: "authorization:daemon-recovery:v1".into(),
            precondition_snapshot_cut_ref: "cut:daemon-recovery:before".into(),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::ExternalSideEffect,
            idempotency_class: ActionIdempotencyClass::Irreversible,
            expected_postcondition_contract_refs: vec!["postcondition:opaque:v1".into()],
        },
    }
}

async fn record_prepared(journal: &ConsequentialJournal, action: &CanonicalActionEnvelope) {
    journal
        .record_intent_admitted(action.clone())
        .await
        .unwrap();
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

#[test]
fn daemon_postcondition_verifier_fails_closed_without_inventing_evidence() {
    let provider = FakeProvider::new();
    let snapshot = provider.build_snapshot("cut:verifier".into());
    let verifier = super::FailClosedWindowsPostconditionVerifier;
    let contract_ref = "postcondition:opaque:v1".to_owned();

    let evidence = verifier
        .verify(
            Uuid::from_u128(0x8404),
            std::slice::from_ref(&contract_ref),
            snapshot.as_ref(),
        )
        .unwrap();

    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].contract_ref, contract_ref);
    assert_eq!(
        evidence[0].status,
        ConsequentialPostconditionStatus::Unknown,
        "legacy/opaque contract must remain unresolved"
    );
    assert!(!evidence[0].receipt_ref.trim().is_empty());
}

#[tokio::test]
async fn boot_debt_recovery_runs_once_per_exact_attachment_and_leaves_opaque_contract_unverified() {
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
    runtime
        .attach(recovery_session(), selection())
        .await
        .unwrap();
    assert_eq!(provider.snapshot_calls(), 1);

    let path = std::env::temp_dir().join(format!(
        "localview-v43-daemon-attachment-recovery-{}.jsonl",
        Uuid::new_v4()
    ));
    let pre_boot_journal = ConsequentialJournal::open(&path).await.unwrap();
    let action = recovery_envelope(&provider);
    record_prepared(&pre_boot_journal, &action).await;
    drop(pre_boot_journal);

    // Crossing the journal reopen boundary is the restart model: durable PREPARED
    // survives, while process-local dispatch grants deliberately do not.
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let scope =
        ConsequentialRecoveryActionScope::from_inventory(&journal.recovery_inventory().await);

    let mut tracker = super::WindowsBootRecoveryTracker::default();
    let verifier = super::FailClosedWindowsPostconditionVerifier;
    let first = super::recover_newly_attached_boot_debt(
        &bridge,
        &journal,
        &runtime,
        &verifier,
        &scope,
        &mut tracker,
    )
    .await;

    assert_eq!(first.len(), 1);
    let drain = first[0]
        .outcome
        .as_ref()
        .expect("first recovery must succeed");
    assert_eq!(drain.entries.len(), 1);
    assert!(matches!(
        &drain.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::Recovered(
            WindowsUiaConsequentialRecoveryOutcome::PostconditionNotVerified {
                action_id,
                world_outcome: WorldOutcome::ReconciliationRequired,
                ..
            }
        ) if *action_id == action.transport_action_id
    ));
    assert_eq!(provider.snapshot_calls(), 2);

    let second = super::recover_newly_attached_boot_debt(
        &bridge,
        &journal,
        &runtime,
        &verifier,
        &scope,
        &mut tracker,
    )
    .await;
    assert!(second.is_empty());
    assert_eq!(provider.snapshot_calls(), 2);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn boot_recovery_scope_excludes_actions_admitted_after_boot_inventory_was_frozen() {
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
    runtime
        .attach(recovery_session(), selection())
        .await
        .unwrap();

    let path = std::env::temp_dir().join(format!(
        "localview-v43-daemon-boot-scope-{}.jsonl",
        Uuid::new_v4()
    ));
    let pre_boot_journal = ConsequentialJournal::open(&path).await.unwrap();
    let boot_action = recovery_envelope(&provider);
    record_prepared(&pre_boot_journal, &boot_action).await;
    drop(pre_boot_journal);

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let boot_scope =
        ConsequentialRecoveryActionScope::from_inventory(&journal.recovery_inventory().await);

    // This action is deliberately admitted after the boot scope was frozen. Its
    // live PREPARED grant stays active, but scoped boot recovery must never touch
    // it and therefore must not race that live dispatch authority.
    let live_action = recovery_envelope(&provider);
    record_prepared(&journal, &live_action).await;

    let mut tracker = super::WindowsBootRecoveryTracker::default();
    let attempts = super::recover_newly_attached_boot_debt(
        &bridge,
        &journal,
        &runtime,
        &super::FailClosedWindowsPostconditionVerifier,
        &boot_scope,
        &mut tracker,
    )
    .await;

    assert_eq!(attempts.len(), 1);
    let drain = attempts[0]
        .outcome
        .as_ref()
        .expect("boot-scoped drain must succeed");
    assert_eq!(drain.entries.len(), 1, "post-boot action must be excluded");
    assert!(matches!(
        &drain.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::Recovered(
            WindowsUiaConsequentialRecoveryOutcome::PostconditionNotVerified { action_id, .. }
        ) if *action_id == boot_action.transport_action_id
    ));
    assert_eq!(
        journal
            .recovery_state(live_action.transport_action_id)
            .await,
        Some(ConsequentialRecoveryState::DispatchPrepared),
        "watcher must not mutate consequential work admitted after boot"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn one_failed_attachment_does_not_starve_later_boot_recovery_and_only_failure_retries() {
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
    runtime
        .attach(recovery_session(), selection())
        .await
        .unwrap();
    runtime
        .attach(second_recovery_session(), second_selection())
        .await
        .unwrap();

    let path = std::env::temp_dir().join(format!(
        "localview-v43-daemon-recovery-fairness-{}.jsonl",
        Uuid::new_v4()
    ));
    let pre_boot_journal = ConsequentialJournal::open(&path).await.unwrap();
    let failing_action = recovery_envelope_for_session(&provider, recovery_session());
    let later_action = recovery_envelope_for_session(&provider, second_recovery_session());
    record_prepared(&pre_boot_journal, &failing_action).await;
    record_prepared(&pre_boot_journal, &later_action).await;
    drop(pre_boot_journal);

    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let scope =
        ConsequentialRecoveryActionScope::from_inventory(&journal.recovery_inventory().await);

    let verifier = SelectiveVerifier {
        failing_action_id: failing_action.transport_action_id,
    };
    let mut tracker = super::WindowsBootRecoveryTracker::default();
    let first = super::recover_newly_attached_boot_debt(
        &bridge,
        &journal,
        &runtime,
        &verifier,
        &scope,
        &mut tracker,
    )
    .await;

    assert_eq!(first.len(), 2, "all exact attachments must get an attempt");
    let failed = first
        .iter()
        .find(|attempt| attempt.session_id == recovery_session())
        .expect("failing lineage must be represented");
    assert!(failed.outcome.is_err());
    let later = first
        .iter()
        .find(|attempt| attempt.session_id == second_recovery_session())
        .expect("later lineage must not be starved");
    let later_drain = later
        .outcome
        .as_ref()
        .expect("later lineage should recover independently");
    assert_eq!(later_drain.entries.len(), 1);
    assert!(matches!(
        &later_drain.entries[0],
        WindowsUiaAttachedRecoveryDrainOutcome::Recovered(
            WindowsUiaConsequentialRecoveryOutcome::PostconditionNotVerified { action_id, .. }
        ) if *action_id == later_action.transport_action_id
    ));

    let second = super::recover_newly_attached_boot_debt(
        &bridge,
        &journal,
        &runtime,
        &verifier,
        &scope,
        &mut tracker,
    )
    .await;
    assert_eq!(second.len(), 1, "only the failed lineage stays retryable");
    assert_eq!(second[0].session_id, recovery_session());
    assert!(second[0].outcome.is_err());

    let _ = std::fs::remove_file(path);
}
