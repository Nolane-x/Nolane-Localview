from pathlib import Path

journal = Path("crates/live-bridge/src/consequential_journal.rs")
text = journal.read_text()
marker = '''    /// Consume one exact observation permit and bind the returned provider
    /// snapshot to its fresh cut. The permit is consumed before validating the
    /// snapshot, so a stale/forged completion cannot be retried with the same
    /// authority.
    pub async fn complete_postcondition_observation(
'''
insert = '''    /// Abandon one exact live postcondition observation authority without
    /// changing durable recovery state or recreating dispatch authority.
    ///
    /// Observation capture is read-only. A failed provider/bridge capture may
    /// therefore release only this process-local grant so recovery can mint a
    /// fresh post-dispatch cut and observe again. The exact permit binding is
    /// required to prevent one caller from clearing another live observation.
    pub async fn abandon_postcondition_observation(
        &self,
        permit: ConsequentialPostconditionObservationPermit,
    ) -> Result<(), ConsequentialPostconditionObservationError> {
        let action_id = permit.action_id;
        if permit.journal_instance_ref != self.journal_instance_ref {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "journal_instance_mismatch",
            });
        }

        let mut state = self.state.lock().await;
        let Some(grant) = state.live_postcondition_observation.get(&action_id) else {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "live_observation_grant_missing",
            });
        };
        if grant.observation_ref != permit.observation_ref
            || grant.session_id != permit.session_id
            || grant.provider_incarnation_ref != permit.provider_incarnation_ref
            || grant.target_incarnation_ref != permit.target_incarnation_ref
            || grant.snapshot_cut_ref != permit.snapshot_cut_ref
            || grant.cause != permit.cause
        {
            return Err(ConsequentialPostconditionObservationError::InvalidPermit {
                action_id,
                reason: "observation_grant_binding_mismatch",
            });
        }

        state.live_postcondition_observation.remove(&action_id);
        Ok(())
    }

''' + marker
if marker not in text:
    raise SystemExit("journal completion marker not found")
text = text.replace(marker, insert, 1)
journal.write_text(text)

runtime = Path("crates/windows-observe-runtime/src/runtime_manager.rs")
text = runtime.read_text()
old_import = '''use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    LiveBridge, ObservationStatus, ProviderIngestReport,
};
'''
new_import = '''use localview_live_bridge::{
    ActionEnvelopeBindingError, ActionEnvelopeMetadata, ActionIdempotencyClass, ActionRiskClass,
    ConsequentialJournal, ConsequentialPostconditionObservationPermit,
    ConsequentialPostconditionObservationReceipt, LiveBridge, ObservationStatus, ProviderIngestReport,
};
'''
if old_import not in text:
    raise SystemExit("runtime live-bridge import marker not found")
text = text.replace(old_import, new_import, 1)

error_marker = '''    #[error("Windows observe LiveBridge state disappeared for session {session_id}")]
    ObservationStateMissing { session_id: SessionId },
}
'''
error_replacement = '''    #[error("Windows observe LiveBridge state disappeared for session {session_id}")]
    ObservationStateMissing { session_id: SessionId },
    #[error("Windows postcondition observation authority failed: {message}")]
    PostconditionObservationAuthority { message: String },
}
'''
if error_marker not in text:
    raise SystemExit("runtime error marker not found")
text = text.replace(error_marker, error_replacement, 1)

method_marker = '''    /// Return the immutable semantic revision currently bound to one attached session.
'''
method = '''    /// Capture one exact journal-authorized post-dispatch semantic snapshot.
    ///
    /// The operation gate serializes the full observation transaction against
    /// attach/drain/reconciliation/release and the real UIA dispatch executor.
    /// Provider or bridge failures abandon only the live observation grant; they
    /// never recreate PREPARED/execution authority and therefore cannot enable a
    /// blind retry of the consequential action.
    pub async fn capture_postcondition_observation(
        &self,
        journal: &ConsequentialJournal,
        permit: ConsequentialPostconditionObservationPermit,
    ) -> Result<ConsequentialPostconditionObservationReceipt, WindowsObserveRuntimeError> {
        let _gate = self.operation_gate.lock().await;
        let session_id = permit.session_id();
        let action_id = permit.action_id();
        let expected_provider = permit.provider_incarnation_ref().clone();
        let expected_target = permit.target_incarnation_ref().clone();
        let expected_cut = permit.snapshot_cut_ref().to_owned();

        let active = {
            let observations = self.active.lock().await;
            observations.get(&session_id).map(|observation| {
                (
                    observation.attachment.clone(),
                    observation.binding.clone(),
                    observation.surface_scope.clone(),
                )
            })
        };
        let Some((attachment, binding, surface_scope)) = active else {
            let failure = WindowsObserveRuntimeError::NotAttached { session_id };
            return Err(abandon_postcondition_capture(journal, permit, failure).await);
        };

        let active_provider = self.provider.provider_incarnation_ref();
        let active_target = self.provider.target_incarnation_ref(&attachment);
        if active_provider != expected_provider
            || binding.provider_incarnation_ref() != &expected_provider
        {
            let failure = WindowsObserveRuntimeError::Provider {
                operation: "postcondition_observation_session_revalidation",
                message: "attached Windows observation provider incarnation does not match the journal permit"
                    .into(),
            };
            return Err(abandon_postcondition_capture(journal, permit, failure).await);
        }
        if active_target != expected_target || binding.target_incarnation_ref() != &expected_target {
            let failure = WindowsObserveRuntimeError::Provider {
                operation: "postcondition_observation_session_revalidation",
                message: "attached Windows observation target incarnation does not match the journal permit"
                    .into(),
            };
            return Err(abandon_postcondition_capture(journal, permit, failure).await);
        }

        let reconciliation_reservation = match self
            .reserve_resource(session_id, ResourceWorkKind::NativeSemanticReconciliation)
            .await
        {
            Ok(reservation) => reservation,
            Err(failure) => {
                return Err(abandon_postcondition_capture(journal, permit, failure).await);
            }
        };

        let provider = self.provider.clone();
        let snapshot_attachment = attachment.clone();
        let snapshot_cut_ref = expected_cut.clone();
        let snapshot = match run_provider("postcondition_snapshot", move || {
            provider.snapshot(&snapshot_attachment, snapshot_cut_ref, surface_scope)
        })
        .await
        {
            Ok(snapshot) => snapshot,
            Err(failure) => {
                drop(reconciliation_reservation);
                return Err(abandon_postcondition_capture(journal, permit, failure).await);
            }
        };

        if snapshot.provider_incarnation_ref() != &expected_provider
            || snapshot.target_incarnation_ref() != &expected_target
            || snapshot.snapshot_cut_ref() != expected_cut
        {
            drop(reconciliation_reservation);
            let failure = WindowsObserveRuntimeError::Provider {
                operation: "postcondition_snapshot_validation",
                message: "provider returned a snapshot outside the exact journal-authorized lineage/cut"
                    .into(),
            };
            return Err(abandon_postcondition_capture(journal, permit, failure).await);
        }

        let receipt_id = format!(
            "reconcile:windows-observe:postdispatch:{session_id}:{action_id}:{}",
            Uuid::new_v4()
        );
        let reconciliation_receipt = snapshot.reconciliation_receipt(receipt_id.clone());
        if let Err(error) = binding
            .record_snapshot_reconciliation(&self.bridge, snapshot.as_ref(), receipt_id)
            .await
        {
            drop(reconciliation_reservation);
            return Err(
                abandon_postcondition_capture(journal, permit, WindowsObserveRuntimeError::Bridge(error))
                    .await,
            );
        }

        self.update_reconciliation_snapshot(session_id, snapshot).await;
        drop(reconciliation_reservation);

        journal
            .complete_postcondition_observation(permit, reconciliation_receipt)
            .await
            .map_err(|error| WindowsObserveRuntimeError::PostconditionObservationAuthority {
                message: error.to_string(),
            })
    }

''' + method_marker
if method_marker not in text:
    raise SystemExit("runtime current snapshot marker not found")
text = text.replace(method_marker, method, 1)

helper_marker = '''fn resource_denial_error(denial: ResourceAdmissionDenial) -> WindowsObserveRuntimeError {
'''
helper = '''async fn abandon_postcondition_capture(
    journal: &ConsequentialJournal,
    permit: ConsequentialPostconditionObservationPermit,
    failure: WindowsObserveRuntimeError,
) -> WindowsObserveRuntimeError {
    match journal.abandon_postcondition_observation(permit).await {
        Ok(()) => failure,
        Err(error) => WindowsObserveRuntimeError::PostconditionObservationAuthority {
            message: format!(
                "postcondition capture failed ({failure}); exact observation abandonment also failed ({error})"
            ),
        },
    }
}

''' + helper_marker
if helper_marker not in text:
    raise SystemExit("runtime resource denial marker not found")
text = text.replace(helper_marker, helper, 1)
runtime.write_text(text)
