use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    LiveBridge,
};
use localview_native_provider::{
    NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, NativeSemanticSnapshotRevision,
    SemanticSnapshotCache, SnapshotBudgetLimit, SnapshotResourceUsage, UserSelectedWindowTarget,
};
use localview_protocol::{
    PrincipalRef, ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef,
    ReconciliationCompleteness, SessionId, TargetIncarnationRef,
};
use localview_windows_observe_runtime::{
    WindowsObserveProvider, WindowsObserveRuntimeConfig, WindowsObserveRuntimeManager,
    WindowsObserveSubscriptionLineage, WindowsUiaActionPreflightError,
    WindowsUiaActionPreflightRequest,
};
use localview_windows_uia_provider::{
    WindowsUiaActionCapabilities, WindowsUiaEventDrain, WindowsUiaPattern, WindowsUiaPatternSupport,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct FakeAttachment(TargetIncarnationRef);

#[derive(Debug, Clone)]
struct FakeSubscription(WindowsObserveSubscriptionLineage);

#[derive(Debug, Clone, Error)]
#[error("fake preflight provider failure")]
struct FakeError;

#[derive(Debug, Default)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    snapshots: usize,
    drains: usize,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    toggle_support: WindowsUiaPatternSupport,
    realization: ProviderElementRealization,
    incomplete: bool,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new(
        toggle_support: WindowsUiaPatternSupport,
        realization: ProviderElementRealization,
        incomplete: bool,
    ) -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:preflight"),
            target: TargetIncarnationRef::from("target:windows:preflight"),
            toggle_support,
            realization,
            incomplete,
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.snapshots, state.drains)
    }

    fn snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("initial snapshot exists")
    }

    fn build_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Toggle, self.toggle_support);
        capabilities.record(
            WindowsUiaPattern::VirtualizedItem,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let node = NativeSemanticNodeObservation {
            element_ref: ProviderElementRef {
                provider_family: "windows_uia".into(),
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                opaque_provider_element_id: "uia-runtime:[48,1]".into(),
                semantic_locator_hints: vec!["automation_id=feature-toggle".into()],
                parent_surface_ref: Some("window:preflight".into()),
                acquisition_cut_ref: cut.clone(),
                realization: self.realization,
                lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
            },
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Enable feature".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("feature-toggle".into()),
            class_name: Some("Button".into()),
            is_enabled: Some(true),
            is_offscreen: Some(false),
            attributes,
        };
        let usage = SnapshotResourceUsage {
            nodes_observed: 1,
            properties_read: 14,
            max_depth_observed: 0,
            exhausted: if self.incomplete {
                vec![SnapshotBudgetLimit::Nodes]
            } else {
                vec![]
            },
            incomplete: self.incomplete,
        };
        let mut cache = SemanticSnapshotCache::for_lineage(self.provider.clone(), self.target.clone());
        cache
            .publish(NativeSemanticSnapshotDraft {
                provider_incarnation_ref: self.provider.clone(),
                target_incarnation_ref: self.target.clone(),
                snapshot_cut_ref: cut,
                surface_scope: "window:preflight".into(),
                cache_profile_revision: "windows-uia-control-view-v1".into(),
                permission_visibility_revision: "windows-uia-interactive-user-v1".into(),
                capture_sequence: 1,
                nodes: vec![node],
                resource_usage: usage,
                completeness: if self.incomplete {
                    ReconciliationCompleteness::Incomplete
                } else {
                    ReconciliationCompleteness::Established
                },
                incompleteness_debt: if self.incomplete {
                    vec!["snapshot_budget_exhausted:Nodes".into()]
                } else {
                    vec![]
                },
            })
            .unwrap()
    }
}

impl WindowsObserveProvider for FakeProvider {
    type Attachment = FakeAttachment;
    type Subscription = FakeSubscription;
    type Error = FakeError;

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
        self.state.lock().unwrap().drains += 1;
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
        let snapshot = self.build_snapshot(snapshot_cut_ref);
        let mut state = self.state.lock().unwrap();
        state.snapshots += 1;
        state.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4801)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x4802,
        expected_process_id: 48,
        selection_nonce: Uuid::from_u128(0x4803),
    }
}

fn build_manager(provider: FakeProvider) -> WindowsObserveRuntimeManager<FakeProvider> {
    WindowsObserveRuntimeManager::new(
        Arc::new(provider),
        LiveBridge::new(64, 8),
        WindowsObserveRuntimeConfig {
            event_capacity: 16,
            drain_limit: 8,
        },
    )
    .unwrap()
}

fn request(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> WindowsUiaActionPreflightRequest {
    WindowsUiaActionPreflightRequest {
        authority: ActionEnvelopeMetadata {
            decision_principal_ref: PrincipalRef::from("principal:decision:preflight"),
            acting_principal_ref: PrincipalRef::from("principal:acting:preflight"),
            authorization_revision: "authorization:preflight:v1".into(),
            precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
            provider_incarnation_ref: provider.provider.clone(),
            target_incarnation_ref: provider.target.clone(),
            risk_class: ActionRiskClass::ReversibleUiState,
            idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
            expected_postcondition_contract_refs: vec!["postcondition:toggle-state".into()],
        },
        element_ref: snapshot.nodes()[0].element_ref.clone(),
        required_pattern: WindowsUiaPattern::Toggle,
    }
}

async fn attached(
    provider: &FakeProvider,
) -> WindowsObserveRuntimeManager<FakeProvider> {
    let runtime = build_manager(provider.clone());
    runtime.attach(session(), selection()).await.unwrap();
    runtime
}

#[tokio::test]
async fn supported_exact_current_realized_evidence_yields_preflight_without_provider_work() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        false,
    );
    let runtime = attached(&provider).await;
    let snapshot = provider.snapshot();
    let before = provider.counts();

    let receipt = runtime
        .preflight_uia_action(session(), request(&provider, &snapshot))
        .await
        .unwrap();

    assert_eq!(receipt.snapshot_cut_ref, snapshot.snapshot_cut_ref());
    assert_eq!(receipt.cache_revision_ref, snapshot.cache_revision_ref());
    assert_eq!(receipt.observed_digest, snapshot.observed_digest());
    assert_eq!(receipt.element_ref, snapshot.nodes()[0].element_ref);
    assert_eq!(receipt.required_pattern, WindowsUiaPattern::Toggle);
    assert_eq!(provider.counts(), before, "preflight must not call provider");
}

#[tokio::test]
async fn unsupported_unknown_and_unrealized_capability_states_fail_closed() {
    for (support, expected) in [
        (
            WindowsUiaPatternSupport::Unsupported,
            WindowsUiaActionPreflightError::PatternUnsupported {
                pattern: WindowsUiaPattern::Toggle,
            },
        ),
        (
            WindowsUiaPatternSupport::Unknown,
            WindowsUiaActionPreflightError::PatternSupportUnknown {
                pattern: WindowsUiaPattern::Toggle,
            },
        ),
    ] {
        let provider = FakeProvider::new(support, ProviderElementRealization::RealizedCurrent, false);
        let runtime = attached(&provider).await;
        let snapshot = provider.snapshot();
        assert_eq!(
            runtime
                .preflight_uia_action(session(), request(&provider, &snapshot))
                .await
                .unwrap_err(),
            expected
        );
    }

    let virtualized = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizationRequired,
        false,
    );
    let runtime = attached(&virtualized).await;
    let snapshot = virtualized.snapshot();
    assert_eq!(
        WindowsUiaActionCapabilities::from_node(&snapshot.nodes()[0])
            .support_for(WindowsUiaPattern::VirtualizedItem),
        WindowsUiaPatternSupport::Supported
    );
    assert_eq!(
        runtime
            .preflight_uia_action(session(), request(&virtualized, &snapshot))
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::ElementNotRealized {
            realization: ProviderElementRealization::RealizationRequired,
        }
    );
}

#[tokio::test]
async fn stale_cut_incomplete_snapshot_unknown_element_and_wrong_lineage_fail_closed() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        false,
    );
    let runtime = attached(&provider).await;
    let snapshot = provider.snapshot();

    let mut stale_cut = request(&provider, &snapshot);
    stale_cut.authority.precondition_snapshot_cut_ref = "cut:stale".into();
    assert!(matches!(
        runtime
            .preflight_uia_action(session(), stale_cut)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::PreconditionSnapshotCutMismatch { .. }
    ));

    let mut missing = request(&provider, &snapshot);
    missing.element_ref.opaque_provider_element_id = "uia-runtime:[999,999]".into();
    assert_eq!(
        runtime
            .preflight_uia_action(session(), missing)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::ElementNotFound
    );

    let mut wrong_provider = request(&provider, &snapshot);
    wrong_provider.authority.provider_incarnation_ref =
        ProviderIncarnationRef::from("provider:windows-uia:stale");
    assert_eq!(
        runtime
            .preflight_uia_action(session(), wrong_provider)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::Authority(
            ActionEnvelopeBindingError::ProviderIncarnationMismatch,
        )
    );

    let mut wrong_target = request(&provider, &snapshot);
    wrong_target.authority.target_incarnation_ref =
        TargetIncarnationRef::from("target:windows:stale");
    assert_eq!(
        runtime
            .preflight_uia_action(session(), wrong_target)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::Authority(
            ActionEnvelopeBindingError::TargetIncarnationMismatch,
        )
    );

    let incomplete = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        true,
    );
    let incomplete_runtime = attached(&incomplete).await;
    let incomplete_snapshot = incomplete.snapshot();
    assert_eq!(
        incomplete_runtime
            .preflight_uia_action(
                session(),
                request(&incomplete, &incomplete_snapshot),
            )
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::SnapshotIncomplete
    );
}