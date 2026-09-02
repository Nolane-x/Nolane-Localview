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
struct FakeAttachment {
    target: TargetIncarnationRef,
}

#[derive(Debug, Clone)]
struct FakeSubscription {
    lineage: WindowsObserveSubscriptionLineage,
}

#[derive(Debug, Clone, Error)]
#[error("fake capability-preflight provider failure")]
struct FakeError;

#[derive(Debug)]
struct FakeState {
    snapshot: Option<Arc<NativeSemanticSnapshotRevision>>,
    snapshot_count: usize,
    drain_count: usize,
}

#[derive(Debug, Clone)]
struct FakeProvider {
    provider: ProviderIncarnationRef,
    target: TargetIncarnationRef,
    support: WindowsUiaPatternSupport,
    realization: ProviderElementRealization,
    incomplete: bool,
    state: Arc<Mutex<FakeState>>,
}

impl FakeProvider {
    fn new(
        support: WindowsUiaPatternSupport,
        realization: ProviderElementRealization,
        incomplete: bool,
    ) -> Self {
        Self {
            provider: ProviderIncarnationRef::from("provider:windows-uia:preflight-contract"),
            target: TargetIncarnationRef::from("target:windows:preflight-contract"),
            support,
            realization,
            incomplete,
            state: Arc::new(Mutex::new(FakeState {
                snapshot: None,
                snapshot_count: 0,
                drain_count: 0,
            })),
        }
    }

    fn counts(&self) -> (usize, usize) {
        let state = self.state.lock().unwrap();
        (state.snapshot_count, state.drain_count)
    }

    fn latest_snapshot(&self) -> Arc<NativeSemanticSnapshotRevision> {
        self.state
            .lock()
            .unwrap()
            .snapshot
            .clone()
            .expect("snapshot must exist")
    }

    fn publish_snapshot(&self, cut: String) -> Arc<NativeSemanticSnapshotRevision> {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        capabilities.record(WindowsUiaPattern::Toggle, self.support);
        capabilities.record(
            WindowsUiaPattern::VirtualizedItem,
            WindowsUiaPatternSupport::Supported,
        );
        let mut attributes = BTreeMap::from([("provider".into(), "windows_uia".into())]);
        capabilities.write_attributes(&mut attributes);

        let element_ref = ProviderElementRef {
            provider_family: "windows_uia".into(),
            provider_incarnation_ref: self.provider.clone(),
            target_incarnation_ref: self.target.clone(),
            opaque_provider_element_id: "uia-runtime:[47,1]".into(),
            semantic_locator_hints: vec!["automation_id=toggle".into()],
            parent_surface_ref: Some("window:preflight-contract".into()),
            acquisition_cut_ref: cut.clone(),
            realization: self.realization,
            lifetime_profile_revision: "windows-uia-lifetime-v1".into(),
        };
        let node = NativeSemanticNodeObservation {
            element_ref,
            parent_index: None,
            depth: 0,
            role: Some("check box".into()),
            name: Some("Enable feature".into()),
            control_type: Some("uia_control_type:50002".into()),
            automation_id: Some("toggle".into()),
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
                surface_scope: "window:preflight-contract".into(),
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
        Ok(FakeAttachment {
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
        Ok(FakeSubscription {
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
        self.state.lock().unwrap().drain_count += 1;
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
        let snapshot = self.publish_snapshot(snapshot_cut_ref);
        let mut state = self.state.lock().unwrap();
        state.snapshot_count += 1;
        state.snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn unsubscribe_events(&self, _subscription: Self::Subscription) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn session() -> SessionId {
    Uuid::from_u128(0x4701)
}

fn selection() -> UserSelectedWindowTarget {
    UserSelectedWindowTarget {
        native_window_handle: 0x4702,
        expected_process_id: 47,
        selection_nonce: Uuid::from_u128(0x4703),
    }
}

fn manager(provider: FakeProvider) -> WindowsObserveRuntimeManager<FakeProvider> {
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

fn authority(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> ActionEnvelopeMetadata {
    ActionEnvelopeMetadata {
        decision_principal_ref: PrincipalRef::from("principal:decision:preflight"),
        acting_principal_ref: PrincipalRef::from("principal:acting:preflight"),
        authorization_revision: "authorization:preflight:v1".into(),
        precondition_snapshot_cut_ref: snapshot.snapshot_cut_ref().into(),
        provider_incarnation_ref: provider.provider.clone(),
        target_incarnation_ref: provider.target.clone(),
        risk_class: ActionRiskClass::ReversibleUiState,
        idempotency_class: ActionIdempotencyClass::IdempotentByObservedState,
        expected_postcondition_contract_refs: vec!["postcondition:toggle-state".into()],
    }
}

fn request(
    provider: &FakeProvider,
    snapshot: &NativeSemanticSnapshotRevision,
) -> WindowsUiaActionPreflightRequest {
    WindowsUiaActionPreflightRequest {
        authority: authority(provider, snapshot),
        element_ref: snapshot.nodes()[0].element_ref.clone(),
        required_pattern: WindowsUiaPattern::Toggle,
    }
}

#[tokio::test]
async fn supported_pattern_on_exact_current_realized_element_yields_preflight_receipt_without_provider_work() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        false,
    );
    let manager = manager(provider.clone());
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();
    let before = provider.counts();

    let receipt = manager
        .preflight_uia_action(session(), request(&provider, &snapshot))
        .await
        .unwrap();

    assert_eq!(receipt.snapshot_cut_ref, snapshot.snapshot_cut_ref());
    assert_eq!(receipt.cache_revision_ref, snapshot.cache_revision_ref());
    assert_eq!(receipt.observed_digest, snapshot.observed_digest());
    assert_eq!(receipt.element_ref, snapshot.nodes()[0].element_ref);
    assert_eq!(receipt.required_pattern, WindowsUiaPattern::Toggle);
    assert_eq!(provider.counts(), before, "preflight must not refresh or invoke UIA");
}

#[tokio::test]
async fn unsupported_and_unknown_pattern_evidence_fail_closed_distinctly() {
    for (support, expected_unknown) in [
        (WindowsUiaPatternSupport::Unsupported, false),
        (WindowsUiaPatternSupport::Unknown, true),
    ] {
        let provider = FakeProvider::new(
            support,
            ProviderElementRealization::RealizedCurrent,
            false,
        );
        let manager = manager(provider.clone());
        manager.attach(session(), selection()).await.unwrap();
        let snapshot = provider.latest_snapshot();
        let error = manager
            .preflight_uia_action(session(), request(&provider, &snapshot))
            .await
            .unwrap_err();

        if expected_unknown {
            assert_eq!(
                error,
                WindowsUiaActionPreflightError::PatternSupportUnknown {
                    pattern: WindowsUiaPattern::Toggle,
                }
            );
        } else {
            assert_eq!(
                error,
                WindowsUiaActionPreflightError::PatternUnsupported {
                    pattern: WindowsUiaPattern::Toggle,
                }
            );
        }
    }
}

#[tokio::test]
async fn virtualized_pattern_availability_does_not_make_an_unrealized_element_actionable() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizationRequired,
        false,
    );
    let manager = manager(provider.clone());
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();

    assert_eq!(
        WindowsUiaActionCapabilities::from_node(&snapshot.nodes()[0])
            .support_for(WindowsUiaPattern::VirtualizedItem),
        WindowsUiaPatternSupport::Supported
    );
    assert_eq!(
        manager
            .preflight_uia_action(session(), request(&provider, &snapshot))
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::ElementNotRealized {
            realization: ProviderElementRealization::RealizationRequired,
        }
    );
}

#[tokio::test]
async fn stale_cut_incomplete_snapshot_and_unknown_exact_element_all_fail_closed() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        false,
    );
    let manager = manager(provider.clone());
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();

    let mut stale_cut = request(&provider, &snapshot);
    stale_cut.authority.precondition_snapshot_cut_ref = "cut:stale".into();
    assert!(matches!(
        manager
            .preflight_uia_action(session(), stale_cut)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::PreconditionSnapshotCutMismatch { .. }
    ));

    let mut unknown_element = request(&provider, &snapshot);
    unknown_element.element_ref.opaque_provider_element_id = "uia-runtime:[999,999]".into();
    assert_eq!(
        manager
            .preflight_uia_action(session(), unknown_element)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::ElementNotFound
    );

    let incomplete_provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        true,
    );
    let incomplete_manager = manager(incomplete_provider.clone());
    incomplete_manager
        .attach(session(), selection())
        .await
        .unwrap();
    let incomplete_snapshot = incomplete_provider.latest_snapshot();
    assert_eq!(
        incomplete_manager
            .preflight_uia_action(
                session(),
                request(&incomplete_provider, &incomplete_snapshot),
            )
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::SnapshotIncomplete
    );
}

#[tokio::test]
async fn provider_and_target_authority_mismatch_reuse_canonical_binding_errors() {
    let provider = FakeProvider::new(
        WindowsUiaPatternSupport::Supported,
        ProviderElementRealization::RealizedCurrent,
        false,
    );
    let manager = manager(provider.clone());
    manager.attach(session(), selection()).await.unwrap();
    let snapshot = provider.latest_snapshot();

    let mut wrong_provider = request(&provider, &snapshot);
    wrong_provider.authority.provider_incarnation_ref =
        ProviderIncarnationRef::from("provider:windows-uia:stale");
    assert_eq!(
        manager
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
        manager
            .preflight_uia_action(session(), wrong_target)
            .await
            .unwrap_err(),
        WindowsUiaActionPreflightError::Authority(
            ActionEnvelopeBindingError::TargetIncarnationMismatch,
        )
    );
}