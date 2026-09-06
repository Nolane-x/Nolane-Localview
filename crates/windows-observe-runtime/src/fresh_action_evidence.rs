#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsFreshActionEvidenceReceipt {
    pub previous_element_ref: ProviderElementRef,
    pub refreshed_element_ref: ProviderElementRef,
    pub snapshot_cut_ref: String,
    pub cache_revision_ref: String,
    pub observed_digest: String,
    pub reconciliation_receipt_ref: String,
}

impl<P: WindowsObserveProvider> WindowsObserveRuntimeManager<P> {
    /// Capture a fresh semantic revision for consequential planning and rebind
    /// the requested provider element only when the exact provider-local identity
    /// still exists in the newly observed revision.
    ///
    /// This operation is observation-only. It mints no action, confirmation,
    /// dispatch lease, PREPARED authority, or execution permit. The operation
    /// gate serializes the entire refresh against attach/drain/reconciliation,
    /// detach, and real provider dispatch so the returned cut is the same cut
    /// installed as the runtime's current immutable snapshot.
    pub async fn refresh_uia_action_evidence(
        &self,
        session_id: SessionId,
        previous_element_ref: ProviderElementRef,
    ) -> Result<WindowsFreshActionEvidenceReceipt, WindowsObserveRuntimeError> {
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

        if previous_element_ref.provider_incarnation_ref != *binding.provider_incarnation_ref()
            || previous_element_ref.target_incarnation_ref != *binding.target_incarnation_ref()
            || previous_element_ref.acquisition_cut_ref != current_snapshot.snapshot_cut_ref()
            || !current_snapshot
                .nodes()
                .iter()
                .any(|node| node.element_ref == previous_element_ref)
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "fresh_action_evidence_current_element_validation",
                message: "requested planning element is not the exact element from the current semantic revision"
                    .into(),
            });
        }

        let active_provider = self.provider.provider_incarnation_ref();
        let active_target = self.provider.target_incarnation_ref(&attachment);
        if active_provider != *binding.provider_incarnation_ref()
            || active_target != *binding.target_incarnation_ref()
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "fresh_action_evidence_session_revalidation",
                message: "attached Windows observation lineage changed before fresh planning observation"
                    .into(),
            });
        }

        let _reconciliation_reservation = self
            .reserve_resource(session_id, ResourceWorkKind::NativeSemanticReconciliation)
            .await?;
        let snapshot_cut_ref = format!(
            "windows-uia:action-plan:{session_id}:{}:{}",
            binding.generation(),
            Uuid::new_v4()
        );
        let provider = self.provider.clone();
        let snapshot_attachment = attachment.clone();
        let expected_cut = snapshot_cut_ref.clone();
        let snapshot = run_provider("fresh_action_evidence_snapshot", move || {
            provider.snapshot(&snapshot_attachment, snapshot_cut_ref, surface_scope)
        })
        .await?;

        if snapshot.provider_incarnation_ref() != binding.provider_incarnation_ref()
            || snapshot.target_incarnation_ref() != binding.target_incarnation_ref()
            || snapshot.snapshot_cut_ref() != expected_cut
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "fresh_action_evidence_snapshot_validation",
                message: "provider returned fresh planning evidence outside the exact attached lineage/cut"
                    .into(),
            });
        }

        let mut matches = snapshot.nodes().iter().filter(|node| {
            node.element_ref.provider_family == previous_element_ref.provider_family
                && node.element_ref.provider_incarnation_ref
                    == previous_element_ref.provider_incarnation_ref
                && node.element_ref.target_incarnation_ref
                    == previous_element_ref.target_incarnation_ref
                && node.element_ref.opaque_provider_element_id
                    == previous_element_ref.opaque_provider_element_id
        });
        let refreshed_element_ref = matches
            .next()
            .map(|node| node.element_ref.clone())
            .ok_or_else(|| WindowsObserveRuntimeError::Provider {
                operation: "fresh_action_evidence_element_rebind",
                message: "exact provider element identity is absent from the fresh planning revision"
                    .into(),
            })?;
        if matches.next().is_some() {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "fresh_action_evidence_element_rebind",
                message: "exact provider element identity is ambiguous in the fresh planning revision"
                    .into(),
            });
        }

        let reconciliation_receipt_ref = format!(
            "reconcile:windows-uia:action-plan:{session_id}:{}:{}",
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

        Ok(WindowsFreshActionEvidenceReceipt {
            previous_element_ref,
            refreshed_element_ref,
            snapshot_cut_ref: snapshot.snapshot_cut_ref().to_owned(),
            cache_revision_ref: snapshot.cache_revision_ref().to_owned(),
            observed_digest: snapshot.observed_digest().to_owned(),
            reconciliation_receipt_ref,
        })
    }
}
