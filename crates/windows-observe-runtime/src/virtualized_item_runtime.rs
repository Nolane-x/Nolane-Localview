use localview_windows_uia_provider::{
    WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaVirtualizedItemQueryRequest,
    WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaVirtualizedItemRealizeRequest,
};

/// Narrow provider capability for UIA ItemContainer/VirtualizedItem work.
///
/// Observe-only providers remain valid without this capability. Implementations
/// may locate and realize a provider-owned placeholder, but the realization
/// receipt itself never upgrades that old reference into current action authority.
pub trait WindowsObserveVirtualizedItemProvider: WindowsObserveProvider {
    fn query_virtualized_item(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, Self::Error>;

    fn realize_virtualized_item(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemRealizeRequest,
    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, Self::Error>;
}

impl WindowsObserveVirtualizedItemProvider for WindowsUiaObserveProvider {
    fn query_virtualized_item(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, Self::Error> {
        self.worker.query_virtualized_item(attachment, request)
    }

    fn realize_virtualized_item(
        &self,
        attachment: &Self::Attachment,
        request: WindowsUiaVirtualizedItemRealizeRequest,
    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, Self::Error> {
        self.worker.realize_virtualized_item(attachment, request)
    }
}

#[derive(Debug, Clone)]
pub struct WindowsVirtualizedItemRefreshReceipt {
    pub query_receipt: WindowsUiaVirtualizedItemQueryReceipt,
    pub realize_receipt: WindowsUiaVirtualizedItemRealizeReceipt,
    pub fresh_snapshot: Arc<NativeSemanticSnapshotRevision>,
    pub reconciliation_receipt_ref: String,
}

impl<P> WindowsObserveRuntimeManager<P>
where
    P: WindowsObserveVirtualizedItemProvider,
{
    /// Realize one exact provider-virtualized item and immediately replace the
    /// runtime's current semantic state with a newly observed reconciliation cut.
    ///
    /// The operation gate covers query -> realize -> fresh snapshot -> publish,
    /// so attach/drain/release or another consequential provider operation cannot
    /// interleave and accidentally turn the pre-realization placeholder into
    /// current authority. No final actionable element is selected by fuzzy name;
    /// callers must reason from the fresh immutable snapshot.
    pub async fn realize_virtualized_item_and_refresh(
        &self,
        session_id: SessionId,
        query_request: WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsVirtualizedItemRefreshReceipt, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let (attachment, binding, surface_scope, current_snapshot) = {
            let observations = self.active.lock().await;
            let observation = observations
                .get(&session_id)
                .ok_or(WindowsObserveRuntimeError::NotAttached { session_id })?;
            (
                observation.attachment.clone(),
                observation.binding.clone(),
                observation.surface_scope.clone(),
                observation.current_snapshot.clone(),
            )
        };

        if current_snapshot.completeness() != ReconciliationCompleteness::Established
            || current_snapshot.resource_usage().incomplete
            || !current_snapshot.incompleteness_debt().is_empty()
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_current_snapshot_incomplete",
                message: "virtualized-item realization requires a complete current semantic revision"
                    .into(),
            });
        }

        let query_cut = query_request.snapshot_cut_ref().to_owned();
        let expected_container = query_request.container_element_ref().clone();
        if query_cut != current_snapshot.snapshot_cut_ref()
            || expected_container.provider_incarnation_ref != *binding.provider_incarnation_ref()
            || expected_container.target_incarnation_ref != *binding.target_incarnation_ref()
            || !current_snapshot
                .nodes()
                .iter()
                .any(|node| node.element_ref == expected_container)
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_current_container_validation",
                message: "ItemContainer query is not bound to the exact container from the current semantic revision"
                    .into(),
            });
        }

        let active_provider = self.provider.provider_incarnation_ref();
        let active_target = self.provider.target_incarnation_ref(&attachment);
        if active_provider != *binding.provider_incarnation_ref()
            || active_target != *binding.target_incarnation_ref()
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_session_revalidation",
                message: "attached Windows observation lineage changed before virtualized-item realization"
                    .into(),
            });
        }

        let provider = self.provider.clone();
        let query_attachment = attachment.clone();
        let query_receipt = run_provider("query_virtualized_item", move || {
            provider.query_virtualized_item(&query_attachment, query_request)
        })
        .await?;
        if query_receipt.snapshot_cut_ref != query_cut
            || query_receipt.provider_incarnation_ref != *binding.provider_incarnation_ref()
            || query_receipt.target_incarnation_ref != *binding.target_incarnation_ref()
            || query_receipt.container_element_ref != expected_container
            || query_receipt.placeholder_element_ref.provider_incarnation_ref
                != *binding.provider_incarnation_ref()
            || query_receipt.placeholder_element_ref.target_incarnation_ref
                != *binding.target_incarnation_ref()
            || query_receipt.placeholder_element_ref.acquisition_cut_ref != query_cut
            || query_receipt.placeholder_element_ref.realization
                != localview_protocol::ProviderElementRealization::RealizationRequired
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_query_receipt_validation",
                message: "provider returned a virtualized placeholder outside the exact current lineage/cut"
                    .into(),
            });
        }

        let realize_request = WindowsUiaVirtualizedItemRealizeRequest::new(
            query_receipt.snapshot_cut_ref.clone(),
            query_receipt.placeholder_element_ref.clone(),
        )
        .map_err(|error| WindowsObserveRuntimeError::Provider {
            operation: "virtualized_item_realize_request",
            message: error.to_string(),
        })?;
        let provider = self.provider.clone();
        let realize_attachment = attachment.clone();
        let realize_receipt = run_provider("realize_virtualized_item", move || {
            provider.realize_virtualized_item(&realize_attachment, realize_request)
        })
        .await?;
        if realize_receipt.provider_incarnation_ref != *binding.provider_incarnation_ref()
            || realize_receipt.target_incarnation_ref != *binding.target_incarnation_ref()
            || realize_receipt.previous_placeholder_ref != query_receipt.placeholder_element_ref
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_realize_receipt_validation",
                message: "provider realization receipt changed the exact placeholder lineage"
                    .into(),
            });
        }

        let _reconciliation_reservation = self
            .reserve_resource(session_id, ResourceWorkKind::NativeSemanticReconciliation)
            .await?;
        let snapshot_cut_ref = format!(
            "windows-uia:virtualized-realize:{session_id}:{}:{}",
            binding.generation(),
            Uuid::new_v4()
        );
        let expected_cut = snapshot_cut_ref.clone();
        let provider = self.provider.clone();
        let snapshot_attachment = attachment.clone();
        let snapshot = run_provider("virtualized_item_fresh_snapshot", move || {
            provider.snapshot(&snapshot_attachment, snapshot_cut_ref, surface_scope)
        })
        .await?;
        if snapshot.provider_incarnation_ref() != binding.provider_incarnation_ref()
            || snapshot.target_incarnation_ref() != binding.target_incarnation_ref()
            || snapshot.snapshot_cut_ref() != expected_cut
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_fresh_snapshot_validation",
                message: "provider returned post-realization evidence outside the exact attached lineage/cut"
                    .into(),
            });
        }

        let reconciliation_receipt_ref = format!(
            "reconcile:windows-uia:virtualized-realize:{session_id}:{}:{}",
            binding.generation(),
            Uuid::new_v4()
        );
        binding
            .record_snapshot_reconciliation(
                &self.bridge,
                snapshot.as_ref(),
                reconciliation_receipt_ref.clone(),
            )
            .await?;
        self.update_reconciliation_snapshot(session_id, snapshot.clone())
            .await;

        if snapshot.completeness() != ReconciliationCompleteness::Established
            || snapshot.resource_usage().incomplete
            || !snapshot.incompleteness_debt().is_empty()
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_fresh_snapshot_incomplete",
                message: format!(
                    "post-realization observation {} is incomplete and cannot establish current authority",
                    snapshot.snapshot_cut_ref()
                ),
            });
        }
        if snapshot
            .nodes()
            .iter()
            .any(|node| node.element_ref == query_receipt.placeholder_element_ref)
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "virtualized_item_stale_placeholder_survived",
                message: "pre-realization placeholder identity survived into the fresh current snapshot"
                    .into(),
            });
        }

        Ok(WindowsVirtualizedItemRefreshReceipt {
            query_receipt,
            realize_receipt,
            fresh_snapshot: snapshot,
            reconciliation_receipt_ref,
        })
    }
}
