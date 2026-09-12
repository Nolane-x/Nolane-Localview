from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# --- execution_arm.rs: one-shot runtime authority + exact receipt binding ---
path = "crates/windows-observe-runtime/src/execution_arm.rs"
replace_once(
    path,
    "    WindowsUiaDispatchContextRequest, WindowsUiaDispatchContextRequirements, WindowsUiaPattern,\n"
    "    WindowsUiaPatternDispatchOperation, evaluate_windows_uia_dispatch_context,\n",
    "    WindowsInputInsertionClass, WindowsUiaDispatchContextRequest,\n"
    "    WindowsUiaDispatchContextRequirements, WindowsUiaPattern, WindowsUiaPatternDispatchOperation,\n"
    "    WindowsUiaVerifiedInputRequest, WindowsVerifiedInputBoundaryReceipt,\n"
    "    WindowsVerifiedKeyboardBatch, classify_windows_input_insertion,\n"
    "    evaluate_windows_uia_dispatch_context,\n",
)

insert_anchor = '''#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaDispatchExecutionResult {
    pub provider_receipt: WindowsUiaProviderExecutionReceipt,
    pub journal_entry: ConsequentialJournalEntry,
}

'''
insert_block = insert_anchor + '''#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputProviderReceipt {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub batch_digest: String,
    pub transport_result: TransportResult,
    pub boundary: WindowsVerifiedInputBoundaryReceipt,
}

#[allow(async_fn_in_trait)]
pub trait WindowsUiaVerifiedInputExecutor: Send + Sync {
    type Error: StdError + Send + Sync + 'static;

    async fn execute_verified_input(
        &self,
        request: WindowsUiaVerifiedInputRequest,
    ) -> Result<WindowsUiaVerifiedInputProviderReceipt, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputExecutionResult {
    pub provider_receipt: WindowsUiaVerifiedInputProviderReceipt,
    pub journal_entry: ConsequentialJournalEntry,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WindowsUiaVerifiedInputExecutionCoordinatorError {
    #[error("Windows verified-input canonical authority changed before provider execution: {message}")]
    PreExecutorAuthorityRejected { message: String },
    #[error("Windows verified-input provider execution failed or became transport-uncertain: {message}")]
    ProviderExecutionFailed { message: String },
    #[error("Windows verified-input provider receipt does not match the exact one-shot request")]
    ProviderReceiptMismatch,
    #[error("Windows verified-input provider returned a receipt without executor delivery")]
    ProviderReceiptTransportMismatch,
    #[error("Windows verified-input execution authority abandonment failed after {stage}: {message}")]
    ExecutionAuthorityAbandonmentFailed {
        stage: &'static str,
        message: String,
    },
    #[error("Windows verified-input durable dispatch linearization append failed: {message}")]
    JournalLinearizationFailed { message: String },
    #[error("Windows verified-input durable linearization entry did not match the exact provider outcome")]
    LinearizationEntryMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsUiaVerifiedInputRequestBinding {
    dispatch_attempt_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    snapshot_cut_ref: String,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    element_ref: ProviderElementRef,
    batch_digest: String,
    batch_event_count: u32,
}

impl WindowsUiaVerifiedInputRequestBinding {
    fn from_request(request: &WindowsUiaVerifiedInputRequest) -> Self {
        Self {
            dispatch_attempt_ref: request.dispatch_attempt_ref(),
            action_id: request.action_id(),
            preparation_journal_sequence: request.preparation_journal_sequence(),
            preparation_receipt_ref: request.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: request.provider_incarnation_ref().clone(),
            target_incarnation_ref: request.target_incarnation_ref().clone(),
            element_ref: request.element_ref().clone(),
            batch_digest: request.batch_digest().to_owned(),
            batch_event_count: request.batch().len() as u32,
        }
    }
}

'''
replace_once(path, insert_anchor, insert_block)

function_anchor = '''async fn verify_prepared_canonical_before_arm(
'''
verified_input_impl = r'''/// Consume the existing opaque Windows execution permit through exactly one
/// verified keyboard fallback attempt and the same durable consequential journal
/// writer used by semantic UIA dispatch.
///
/// The caller never supplies provider lineage or a raw platform request. The
/// coordinator derives those fields from the sealed PREPARED authority, binds the
/// exact ordered key batch to that one-shot permit, moves the request into the
/// executor once, validates exact receipt identity and raw insertion consistency,
/// then linearizes the classified dispatch result. Full insertion is dispatch
/// evidence only; world success remains a later reconciliation decision.
pub async fn execute_armed_uia_verified_input<E>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    batch: WindowsVerifiedKeyboardBatch,
    executor: &E,
) -> Result<WindowsUiaVerifiedInputExecutionResult, WindowsUiaVerifiedInputExecutionCoordinatorError>
where
    E: WindowsUiaVerifiedInputExecutor,
{
    if let Err(error) =
        verify_armed_canonical_before_executor(bridge, journal, session_id, &armed).await
    {
        journal
            .abandon_dispatch_execution(armed.dispatch_permit)
            .await
            .map_err(|abandonment| {
                WindowsUiaVerifiedInputExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {
                    stage: "pre_executor_revalidation_failed",
                    message: abandonment.to_string(),
                }
            })?;
        return Err(
            WindowsUiaVerifiedInputExecutionCoordinatorError::PreExecutorAuthorityRejected {
                message: error.to_string(),
            },
        );
    }

    let lease = &armed.seal.authority.dispatch_revalidation.element_lease;
    let request = WindowsUiaVerifiedInputRequest::from_execution_permit(
        &armed.dispatch_permit,
        lease.snapshot_cut_ref.clone(),
        lease.provider_incarnation_ref.clone(),
        lease.target_incarnation_ref.clone(),
        lease.element_ref.clone(),
        armed.seal.context.requirements,
        batch,
    );
    let binding = WindowsUiaVerifiedInputRequestBinding::from_request(&request);
    let action_id = binding.action_id;

    let provider_receipt = match executor.execute_verified_input(request).await {
        Ok(receipt) => receipt,
        Err(error) => {
            let message = error.to_string();
            journal
                .abandon_dispatch_execution(armed.dispatch_permit)
                .await
                .map_err(|abandonment| {
                    WindowsUiaVerifiedInputExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {
                        stage: "provider_execution_failed",
                        message: abandonment.to_string(),
                    }
                })?;
            return Err(
                WindowsUiaVerifiedInputExecutionCoordinatorError::ProviderExecutionFailed {
                    message,
                },
            );
        }
    };

    if !verified_input_receipt_matches_request(&provider_receipt, &binding) {
        journal
            .abandon_dispatch_execution(armed.dispatch_permit)
            .await
            .map_err(|abandonment| {
                WindowsUiaVerifiedInputExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {
                    stage: "provider_receipt_mismatch",
                    message: abandonment.to_string(),
                }
            })?;
        return Err(WindowsUiaVerifiedInputExecutionCoordinatorError::ProviderReceiptMismatch);
    }
    if provider_receipt.transport_result != TransportResult::DeliveredToExecutor {
        journal
            .abandon_dispatch_execution(armed.dispatch_permit)
            .await
            .map_err(|abandonment| {
                WindowsUiaVerifiedInputExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {
                    stage: "provider_transport_mismatch",
                    message: abandonment.to_string(),
                }
            })?;
        return Err(
            WindowsUiaVerifiedInputExecutionCoordinatorError::ProviderReceiptTransportMismatch,
        );
    }

    let boundary = &provider_receipt.boundary;
    let classified = classify_windows_input_insertion(
        boundary.requested_event_count,
        boundary.inserted_event_count,
    )
    .ok();
    if boundary.requested_event_count != binding.batch_event_count
        || classified != Some(boundary.insertion_class)
        || (matches!(
            boundary.insertion_class,
            WindowsInputInsertionClass::FullyInserted
                | WindowsInputInsertionClass::PartialDispatchUnknownOutcome
        ) && !boundary.reconciliation_required)
    {
        journal
            .abandon_dispatch_execution(armed.dispatch_permit)
            .await
            .map_err(|abandonment| {
                WindowsUiaVerifiedInputExecutionCoordinatorError::ExecutionAuthorityAbandonmentFailed {
                    stage: "provider_boundary_mismatch",
                    message: abandonment.to_string(),
                }
            })?;
        return Err(WindowsUiaVerifiedInputExecutionCoordinatorError::ProviderReceiptMismatch);
    }

    let dispatch_result = crate::dispatch_result_for_verified_input(boundary);
    let linearization = DispatchLinearizationReceipt {
        receipt_ref: format!(
            "windows-uia:verified-input:{}:{}",
            provider_receipt.dispatch_attempt_ref, provider_receipt.batch_digest
        ),
        transport_result: provider_receipt.transport_result,
        dispatch_result,
    };
    let journal_entry = journal
        .record_dispatch_linearized(armed.dispatch_permit, linearization.clone())
        .await
        .map_err(|error| {
            WindowsUiaVerifiedInputExecutionCoordinatorError::JournalLinearizationFailed {
                message: error.to_string(),
            }
        })?;

    if journal_entry.action_id != action_id
        || !matches!(
            &journal_entry.transition,
            ConsequentialJournalTransition::DispatchLinearized { receipt }
                if receipt == &linearization
        )
    {
        return Err(WindowsUiaVerifiedInputExecutionCoordinatorError::LinearizationEntryMismatch);
    }

    Ok(WindowsUiaVerifiedInputExecutionResult {
        provider_receipt,
        journal_entry,
    })
}

fn verified_input_receipt_matches_request(
    receipt: &WindowsUiaVerifiedInputProviderReceipt,
    request: &WindowsUiaVerifiedInputRequestBinding,
) -> bool {
    receipt.dispatch_attempt_ref == request.dispatch_attempt_ref
        && receipt.action_id == request.action_id
        && receipt.preparation_journal_sequence == request.preparation_journal_sequence
        && receipt.preparation_receipt_ref == request.preparation_receipt_ref
        && receipt.snapshot_cut_ref == request.snapshot_cut_ref
        && receipt.provider_incarnation_ref == request.provider_incarnation_ref
        && receipt.target_incarnation_ref == request.target_incarnation_ref
        && receipt.element_ref == request.element_ref
        && receipt.batch_digest == request.batch_digest
}

'''
replace_once(path, function_anchor, verified_input_impl + function_anchor)

# --- runtime_manager.rs: production adapter on same operation gate ---
path = "crates/windows-observe-runtime/src/runtime_manager.rs"
insert_anchor = '''

impl crate::WindowsUiaSetValueExecutor for WindowsUiaRuntimeDispatchExecutor {
'''
impl_block = r'''

impl crate::WindowsUiaVerifiedInputExecutor for WindowsUiaRuntimeDispatchExecutor {
    type Error = WindowsObserveRuntimeError;

    async fn execute_verified_input(
        &self,
        request: localview_windows_uia_provider::WindowsUiaVerifiedInputRequest,
    ) -> Result<crate::WindowsUiaVerifiedInputProviderReceipt, Self::Error> {
        if request.provider_incarnation_ref() != &self.provider_incarnation_ref
            || request.target_incarnation_ref() != &self.target_incarnation_ref
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "dispatch_verified_input_executor_lineage_validation",
                message: "verified-input request lineage differs from the resolved runtime executor"
                    .into(),
            });
        }

        let _gate = self.operation_gate.lock().await;
        let attachment = self
            .active
            .lock()
            .await
            .get(&self.session_id)
            .map(|observation| observation.attachment.clone())
            .ok_or(WindowsObserveRuntimeError::NotAttached {
                session_id: self.session_id,
            })?;
        if attachment.provider_incarnation_ref() != &self.provider_incarnation_ref
            || attachment.target_incarnation_ref() != &self.target_incarnation_ref
        {
            return Err(WindowsObserveRuntimeError::Provider {
                operation: "dispatch_verified_input_session_revalidation",
                message: "attached Windows UIA session lineage changed after verified-input executor resolution"
                    .into(),
            });
        }

        let provider = self.provider.clone();
        let dispatch_attachment = attachment.clone();
        let receipt = run_provider("dispatch_verified_input", move || {
            provider
                .worker
                .dispatch_verified_input(&dispatch_attachment, request)
        })
        .await?;

        Ok(crate::WindowsUiaVerifiedInputProviderReceipt {
            dispatch_attempt_ref: receipt.dispatch_attempt_ref(),
            action_id: receipt.action_id(),
            preparation_journal_sequence: receipt.preparation_journal_sequence(),
            preparation_receipt_ref: receipt.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref: receipt.snapshot_cut_ref().to_owned(),
            provider_incarnation_ref: receipt.provider_incarnation_ref().clone(),
            target_incarnation_ref: receipt.target_incarnation_ref().clone(),
            element_ref: receipt.element_ref().clone(),
            batch_digest: receipt.batch_digest().to_owned(),
            transport_result: localview_protocol::TransportResult::DeliveredToExecutor,
            boundary: receipt.boundary().clone(),
        })
    }
}
'''
replace_once(path, insert_anchor, impl_block + insert_anchor)

# Cross-platform API parity: non-Windows runtime compiles the production adapter
# but fails closed before any side effect.
path = "crates/windows-uia-provider/src/subscription_stub.rs"
replace_once(
    path,
    "    WindowsUiaSetValueVerificationReceipt, WindowsUiaSetValueVerificationRequest,\n",
    "    WindowsUiaSetValueVerificationReceipt, WindowsUiaSetValueVerificationRequest,\n"
    "    WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,\n",
)
insert_anchor = '''    pub fn verify_set_value(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaSetValueVerificationRequest,
    ) -> Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError> {
        self.inner.verify_set_value(attachment, request)
    }

'''
insert_block = insert_anchor + '''    pub fn dispatch_verified_input(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: WindowsUiaVerifiedInputRequest,
    ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

'''
replace_once(path, insert_anchor, insert_block)

# --- verified_execution.rs: one shared world-verification core ---
path = "crates/windows-observe-runtime/src/verified_execution.rs"
p = Path(path)
text = p.read_text(encoding="utf-8")
text = text.replace(
    "    WindowsUiaDispatchExecutor, execute_armed_uia_dispatch,\n",
    "    WindowsUiaDispatchExecutor, WindowsUiaVerifiedInputExecutionCoordinatorError,\n"
    "    WindowsUiaVerifiedInputExecutor, execute_armed_uia_dispatch,\n"
    "    execute_armed_uia_verified_input,\n",
    1,
)
text = text.replace(
    "use localview_protocol::{DispatchResult, SessionId, WorldOutcome};\n",
    "use localview_protocol::{DispatchResult, SessionId, WorldOutcome};\n"
    "use localview_windows_uia_provider::WindowsVerifiedKeyboardBatch;\n",
    1,
)
text = text.replace(
    "    #[error(transparent)]\n    Dispatch(#[from] WindowsUiaDispatchExecutionCoordinatorError),\n",
    "    #[error(transparent)]\n    Dispatch(#[from] WindowsUiaDispatchExecutionCoordinatorError),\n"
    "    #[error(transparent)]\n"
    "    VerifiedInputDispatch(#[from] WindowsUiaVerifiedInputExecutionCoordinatorError),\n",
    1,
)
start_marker = "pub async fn execute_armed_uia_dispatch_verified<P, E, V>(\n"
end_marker = "/// Recover one consequential UIA action using only durable journal authority.\n"
start = text.find(start_marker)
end = text.find(end_marker)
if start < 0 or end < 0 or end <= start:
    raise SystemExit("verified_execution.rs: failed to locate dispatch verification segment")
# Keep the doc comment immediately before the function by locating its start.
doc_start = text.rfind("/// Execute one armed consequential UIA action through world verification.\n", 0, start)
if doc_start < 0:
    raise SystemExit("verified_execution.rs: dispatch verification doc anchor missing")
new_segment = r'''/// Execute one armed consequential semantic UIA action through world verification.
pub async fn execute_armed_uia_dispatch_verified<P, E, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    executor: &E,
    verifier: &V,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaVerifiedExecutionError>
where
    P: WindowsObserveProvider,
    E: WindowsUiaDispatchExecutor,
    V: WindowsUiaPostconditionVerifier,
{
    let dispatch = execute_armed_uia_dispatch(bridge, journal, session_id, armed, executor).await?;
    complete_uia_dispatch_verification(
        bridge,
        journal,
        runtime,
        session_id,
        dispatch.provider_receipt.action_id,
        dispatch.provider_receipt.dispatch_result,
        dispatch.journal_entry.journal_sequence,
        verifier,
    )
    .await
}

/// Execute one armed verified-keyboard fallback through the exact same
/// post-dispatch observation, predicate verification, reconciliation and commit
/// path as semantic UIA. Full platform insertion is therefore never world
/// success by itself, and partial insertion remains reconciliation-only.
pub async fn execute_armed_uia_verified_input_verified<P, E, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    armed: WindowsUiaDispatchExecutionPermit,
    batch: WindowsVerifiedKeyboardBatch,
    executor: &E,
    verifier: &V,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaVerifiedExecutionError>
where
    P: WindowsObserveProvider,
    E: WindowsUiaVerifiedInputExecutor,
    V: WindowsUiaPostconditionVerifier,
{
    let dispatch =
        execute_armed_uia_verified_input(bridge, journal, session_id, armed, batch, executor).await?;
    complete_uia_dispatch_verification(
        bridge,
        journal,
        runtime,
        session_id,
        dispatch.provider_receipt.action_id,
        crate::dispatch_result_for_verified_input(&dispatch.provider_receipt.boundary),
        dispatch.journal_entry.journal_sequence,
        verifier,
    )
    .await
}

async fn complete_uia_dispatch_verification<P, V>(
    bridge: &LiveBridge,
    journal: &ConsequentialJournal,
    runtime: &WindowsObserveRuntimeManager<P>,
    session_id: SessionId,
    action_id: Uuid,
    dispatch_result: DispatchResult,
    dispatch_journal_sequence: u64,
    verifier: &V,
) -> Result<WindowsUiaVerifiedExecutionOutcome, WindowsUiaVerifiedExecutionError>
where
    P: WindowsObserveProvider,
    V: WindowsUiaPostconditionVerifier,
{
    let state = journal.recovery_state(action_id).await;
    if state == Some(ConsequentialRecoveryState::KnownNotDispatched) {
        return Ok(WindowsUiaVerifiedExecutionOutcome::KnownNotDispatched {
            action_id,
            dispatch_result,
            dispatch_journal_sequence,
        });
    }
    if state != Some(ConsequentialRecoveryState::PossiblyDispatched) {
        return Err(WindowsUiaVerifiedExecutionError::UnexpectedRecoveryState { state });
    }

    let permit = journal
        .begin_postcondition_observation(action_id)
        .await
        .map_err(
            |error| WindowsUiaVerifiedExecutionError::ObservationAuthority {
                message: error.to_string(),
            },
        )?;
    let capture = runtime
        .capture_postcondition_observation_with_snapshot(journal, permit)
        .await
        .map_err(|error| WindowsUiaVerifiedExecutionError::Capture {
            message: error.to_string(),
        })?;
    let observation = capture.observation_receipt();
    let snapshot = capture.snapshot();

    if snapshot.snapshot_cut_ref() != observation.snapshot_cut_ref()
        || snapshot.provider_incarnation_ref() != observation.provider_incarnation_ref()
        || snapshot.target_incarnation_ref() != observation.target_incarnation_ref()
    {
        return Err(WindowsUiaVerifiedExecutionError::SnapshotBindingMismatch);
    }

    let envelope = journal
        .entries_for(action_id)
        .await
        .into_iter()
        .find_map(|entry| match entry.transition {
            ConsequentialJournalTransition::IntentAdmitted { envelope } => Some(envelope),
            _ => None,
        })
        .ok_or(WindowsUiaVerifiedExecutionError::AdmittedEnvelopeMissing { action_id })?;
    if envelope.transport_action_id != action_id || envelope.session_id != session_id {
        return Err(WindowsUiaVerifiedExecutionError::AdmittedEnvelopeMismatch);
    }

    let evidence = verifier
        .verify(
            action_id,
            &envelope.metadata.expected_postcondition_contract_refs,
            snapshot.as_ref(),
        )
        .map_err(|error| WindowsUiaVerifiedExecutionError::Verifier {
            message: error.to_string(),
        })?;

    let reconciliation = reconcile_consequential_postconditions(
        bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            capture.into_observation_receipt(),
            evidence,
        ),
    )
    .await
    .map_err(|error| WindowsUiaVerifiedExecutionError::Reconciliation {
        message: error.to_string(),
    })?;

    let reconciliation_journal_sequence = reconciliation.journal_entry.journal_sequence;
    if reconciliation.world_outcome == WorldOutcome::VerifiedExpected
        && reconciliation.postconditions_verified
    {
        let commit = journal.record_committed(action_id).await.map_err(|error| {
            WindowsUiaVerifiedExecutionError::Commit {
                message: error.to_string(),
            }
        })?;
        return Ok(WindowsUiaVerifiedExecutionOutcome::Committed {
            action_id,
            world_outcome: reconciliation.world_outcome,
            dispatch_journal_sequence,
            reconciliation_journal_sequence,
            commit_journal_sequence: commit.journal_sequence,
        });
    }

    Ok(WindowsUiaVerifiedExecutionOutcome::PostconditionNotVerified {
        action_id,
        world_outcome: reconciliation.world_outcome,
        dispatch_journal_sequence,
        reconciliation_journal_sequence,
    })
}

'''
text = text[:doc_start] + new_segment + text[end:]
p.write_text(text, encoding="utf-8")

# Focused proof must execute Task 4 explicitly on every future connector push.
path = ".github/workflows/v43-green-proof.yml"
replace_once(
    path,
    "      - name: Run verified input runtime dispatch classification\n"
    "        run: cargo test -p localview-windows-observe-runtime --test verified_input_dispatch_classification -- --nocapture\n",
    "      - name: Run verified input runtime dispatch classification\n"
    "        run: cargo test -p localview-windows-observe-runtime --test verified_input_dispatch_classification -- --nocapture\n"
    "      - name: Run verified input runtime authority contract\n"
    "        run: cargo test -p localview-windows-observe-runtime --test verified_input_execution_contract -- --nocapture\n"
    "      - name: Run shared execution coordinator behavior\n"
    "        run: cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior -- --nocapture\n",
)

print("Task 4 verified-input runtime GREEN patch applied")
