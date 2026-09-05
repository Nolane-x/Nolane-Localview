use std::{
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass, CanonicalActionEnvelope,
    ConsequentialJournal, ConsequentialJournalError, DispatchLinearizationReceipt,
    DispatchPreparationReceipt, LiveBridge,
};
use localview_native_provider::{
    NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision, SemanticSnapshotCache,
    SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    DispatchResult, PrincipalRef, ProviderIncarnationRef, ReconciliationCompleteness, SessionId,
    TargetIncarnationRef, TransportResult,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage,
};
use localview_windows_uia_provider::WindowsUiaEventDrain;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Attachment {
    target: TargetIncarnationRef,
}

#[derive(Debug, Clone)]
struct Subscription {
    lineage: WindowsObserveSubscriptionLineage,
}

#[derive(Debug, Clone, Error)]
#[error("snapshot failed")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    requested_cuts: Vec<String>,
    fail_next_snapshot: bool,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new() -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:uia:postcondition-runtime:1"),
            target: TargetIncarnationRef::from("target:window:postcondition-runtime:1"),
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn fail_next_snapshot(&self) {
        self.state.lock().unwrap().fail_next_snapshot = true;
    }

    fn requested_cuts(&self) -> Vec<String> {
        self.state.lock().unwrap().requested_cuts.clone()
    }
}

impl WindowsObserveProvider for FakeProvider {
    type Attachment = Attachment;
    type Subscription = Subscription;
    type Error = FakeError;

    fn provider_incarnation_ref(&self) -> ProviderIncarnationRef {
        self.provider.clone()
    }

    fn attach(&self, _selection: UserSelectedWindowTarget) -> Result<Self::Attachment, Self::Error> {
        Ok(Attachment {
            target: self.target.clone(),
        })
    }

    fn target_incarnation_ref(&self, attachment: &Self::Attachment) -> TargetIncarnationRef {
        attachment.target.clone()
    }

    fn subscribe_events(
        &self,
        attachment: &Self::Attachment,
        _capacity: usize,
    ) -> Result<Self::Subscription, Self::Error> {
        Ok(Subscription {
            lineage: WindowsObserveSubscriptionLineage {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: attachment.target.clone(),
                sequence_baseline: 0,
            },
        })
    }

    fn subscription_lineage(
        &self,
        subscription: &Self::Subscription,
    ) -> WindowsObserveSubscriptionLineage {
        subscription.lineage.clone()
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
        surface_scope: String,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, Self::Error> {
        let sequence = {
            let mut state = self.state.lock().unwrap();
            state.requested_cuts.push(snapshot_cut_ref.clone());
            if state.fail_next_snapshot {
                state.fail_next_snapshot = false;
                return Err(FakeError);
            }
            state.requested_cuts.len() as u64
        };
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        Ok(cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref,
                surface_scope,
                cache_profile_revision: "cache:postcondition-runtime:v1".into(),
                permission_visibility_revision: "permission:postcondition-runtime:v1".into(),
                capture_sequence: sequence,
                nodes: vec![],
                resource_usage: SnapshotResourceUsage {
                    nodes_observed: 0,
                    properties_read: 0,
                    max_depth_observed: 0,
                    exhausted: vec![],
                    incomplete: false,
                },
                completeness: ReconciliationCompleteness::Established,
                incompleteness_debt: vec![],
            })
            .unwrap())
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x7201)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x7202,
        expected_process_id: 72,
        selection_nonce: Uuid::from_u128(0x7203),
    }
}

fn journal_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
}

fn action(provider: &FakeProvider) -> CanonicalActionEnvelope {
    CanonicalActionEnvelope {
        envelope_id: Uuid::new_v4(),
        transport_action_id: Uuid::new_v4(),
        session_id: session(),
        metadata: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:planner:postcondition-runtime"),
            acting_principal_ref: PrincipalRef::from("principal:executor:postcondition-runtime"),
            authorization_revision: "auth:postcondition-runtime:v1".into(),
            precondition_snapshot_cut_ref: "cut:pre-dispatch:postcondition-runtime".into(),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["post:runtime-visible".into()],
        },
    }
}

async fn linearize(journal: &ConsequentialJournal, action: &CanonicalActionEnvelope) {
    journal.record_intent_admitted(action.clone()).await.unwrap();
    let authorization = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap();
    let prepared = journal
        .record_dispatch_prepared(
            action.transport_action_id,
            DispatchPreparationReceipt {
                receipt_ref: "prepared:postcondition-runtime".into(),
                authorization_journal_sequence: authorization.journal_sequence,
                precondition_snapshot_cut_ref: action.metadata.precondition_snapshot_cut_ref.clone(),
                provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            },
        )
        .await
        .unwrap();
    let (_, capability) = prepared.into_parts();
    let dispatch = journal.begin_dispatch(capability).await.unwrap();
    journal
        .record_dispatch_linearized(
            dispatch,
            DispatchLinearizationReceipt {
                receipt_ref: "dispatch:postcondition-runtime".into(),
                transport_result: TransportResult::DeliveredToExecutor,
                dispatch_result: DispatchResult::DispatchedFull,
            },
        )
        .await
        .unwrap();
}

async fn runtime(
    provider: FakeProvider,
    bridge: LiveBridge,
) -> WindowsObserveRuntimeManager<FakeProvider> {
    let manager = WindowsObserveRuntimeManager::new(
        Arc::new(provider),
        bridge,
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap();
    manager.attach(session(), selection()).await.unwrap();
    manager
}

#[tokio::test]
async fn runtime_owns_exact_post_dispatch_snapshot_capture_and_journal_completion() {
    let path = journal_path("runtime-postcondition-capture");
    let provider = FakeProvider::new();
    let bridge = LiveBridge::new(32, 8);
    let manager = runtime(provider.clone(), bridge).await;
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let action = action(&provider);
    linearize(&journal, &action).await;

    let permit = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let expected_cut = permit.snapshot_cut_ref().to_owned();
    let receipt = manager
        .capture_postcondition_observation(&journal, permit)
        .await
        .expect("runtime must own provider capture and journal completion");

    assert_eq!(receipt.action_id(), action.transport_action_id);
    assert_eq!(receipt.session_id(), session());
    assert_eq!(receipt.snapshot_cut_ref(), expected_cut);
    assert_ne!(
        receipt.snapshot_cut_ref(),
        action.metadata.precondition_snapshot_cut_ref
    );
    assert_eq!(provider.requested_cuts().last(), Some(&expected_cut));

    let current = manager
        .current_semantic_snapshot(session())
        .await
        .expect("runtime must retain the exact provider snapshot before completing authority");
    assert_eq!(current.snapshot_cut_ref(), expected_cut);
    let status = manager
        .status(session())
        .await
        .expect("runtime must publish the exact reconciliation receipt to LiveBridge");
    assert_eq!(
        status.reconciliation_receipt_id.as_deref(),
        Some(receipt.reconciliation_receipt_ref())
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn failed_provider_observation_is_safely_abandoned_without_reopening_dispatch() {
    let path = journal_path("runtime-postcondition-failure");
    let provider = FakeProvider::new();
    let bridge = LiveBridge::new(32, 8);
    let manager = runtime(provider.clone(), bridge).await;
    let journal = ConsequentialJournal::open(&path).await.unwrap();
    let action = action(&provider);
    linearize(&journal, &action).await;

    let first = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let first_cut = first.snapshot_cut_ref().to_owned();
    provider.fail_next_snapshot();
    manager
        .capture_postcondition_observation(&journal, first)
        .await
        .expect_err("provider snapshot failure must remain an observation failure");

    let second = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .expect("failed non-side-effect observation must release its live grant");
    assert_ne!(second.snapshot_cut_ref(), first_cut);
    journal
        .abandon_postcondition_observation(second)
        .await
        .expect("explicit observation abandonment must be safe and exact");

    let retry = journal
        .record_authorization(
            action.transport_action_id,
            action.metadata.authorization_revision.clone(),
            true,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        retry,
        ConsequentialJournalError::InvalidTransition { .. }
    ));

    let _ = std::fs::remove_file(path);
}
