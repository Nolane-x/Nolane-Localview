from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# 1. Provider request contract: authority lineage + exact batch.
replace_once(
    "crates/windows-uia-provider/src/verified_input.rs",
    "use thiserror::Error;\n",
    "use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};\n"
    "use thiserror::Error;\n"
    "use uuid::Uuid;\n",
)
replace_once(
    "crates/windows-uia-provider/src/verified_input.rs",
    "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct WindowsKeyboardStateSnapshot {",
    "#[derive(Debug, PartialEq, Eq)]\n"
    "pub struct WindowsUiaVerifiedInputRequest {\n"
    "    pub dispatch_attempt_ref: Uuid,\n"
    "    pub action_id: Uuid,\n"
    "    pub preparation_journal_sequence: u64,\n"
    "    pub preparation_receipt_ref: String,\n"
    "    pub snapshot_cut_ref: String,\n"
    "    pub provider_incarnation_ref: ProviderIncarnationRef,\n"
    "    pub target_incarnation_ref: TargetIncarnationRef,\n"
    "    pub element_ref: ProviderElementRef,\n"
    "    pub context_requirements: WindowsUiaDispatchContextRequirements,\n"
    "    pub batch: WindowsVerifiedKeyboardBatch,\n"
    "}\n\n"
    "#[derive(Debug, Clone, PartialEq, Eq)]\n"
    "pub struct WindowsKeyboardStateSnapshot {",
)

# 2. Exact UIA MTA owns the volatile revalidation + modifier snapshot + insertion.
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "    #[error(\"Windows UI Automation SetValue verification request is invalid\")]\n"
    "    InvalidSetValueVerificationRequest,\n",
    "    #[error(\"Windows UI Automation SetValue verification request is invalid\")]\n"
    "    InvalidSetValueVerificationRequest,\n"
    "    #[error(\"Windows UI Automation verified input request is invalid\")]\n"
    "    InvalidVerifiedInputRequest,\n"
    "    #[error(\"Windows UI Automation verified input boundary failed: {0}\")]\n"
    "    VerifiedInputBoundary(#[from] crate::WindowsVerifiedInputBoundaryError),\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        WindowsUiaSetValueVerificationRequest, WindowsUiaValueCapabilityFacts,\n"
    "        WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaVirtualizedItemQueryRequest,\n",
    "        WindowsUiaSetValueVerificationRequest, WindowsUiaValueCapabilityFacts,\n"
    "        WindowsInputDispatchBlocker, WindowsUiaVerifiedInputRequest,\n"
    "        WindowsVerifiedInputBoundaryError, WindowsVerifiedInputBoundaryReceipt,\n"
    "        WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaVirtualizedItemQueryRequest,\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        evaluate_windows_uia_dispatch_context,\n"
    "    };",
    "        classify_windows_input_insertion, evaluate_windows_keyboard_state,\n"
    "        evaluate_windows_uia_dispatch_context, snapshot_windows_keyboard_state,\n"
    "        windows_insert_verified_key_events,\n"
    "    };",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        VerifySetValue {\n"
    "            attachment: WindowsUiaAttachment,\n"
    "            request: WindowsUiaSetValueVerificationRequest,\n"
    "            reply: Sender<Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError>>,\n"
    "        },\n",
    "        DispatchVerifiedInput {\n"
    "            attachment: WindowsUiaAttachment,\n"
    "            request: WindowsUiaVerifiedInputRequest,\n"
    "            reply: Sender<Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError>>,\n"
    "        },\n"
    "        VerifySetValue {\n"
    "            attachment: WindowsUiaAttachment,\n"
    "            request: WindowsUiaSetValueVerificationRequest,\n"
    "            reply: Sender<Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError>>,\n"
    "        },\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        pub(crate) fn query_virtualized_item_on_mta(\n",
    "        pub fn dispatch_verified_input(\n"
    "            &self,\n"
    "            attachment: &WindowsUiaAttachment,\n"
    "            request: WindowsUiaVerifiedInputRequest,\n"
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n"
    "            if request.dispatch_attempt_ref.is_nil()\n"
    "                || request.action_id.is_nil()\n"
    "                || request.preparation_journal_sequence == 0\n"
    "                || request.preparation_receipt_ref.trim().is_empty()\n"
    "                || request.snapshot_cut_ref.trim().is_empty()\n"
    "                || request.provider_incarnation_ref != self.provider_incarnation_ref\n"
    "                || request.provider_incarnation_ref != attachment.provider_incarnation_ref\n"
    "                || request.target_incarnation_ref != attachment.target_incarnation_ref\n"
    "                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref\n"
    "                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref\n"
    "                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref\n"
    "            {\n"
    "                return Err(WindowsUiaWorkerError::InvalidVerifiedInputRequest);\n"
    "            }\n"
    "            let (reply_tx, reply_rx) = mpsc::channel();\n"
    "            self.ensure_healthy()?;\n"
    "            self.sender\n"
    "                .send(WorkerCommand::DispatchVerifiedInput {\n"
    "                    attachment: attachment.clone(),\n"
    "                    request,\n"
    "                    reply: reply_tx,\n"
    "                })\n"
    "                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;\n"
    "            self.receive(&reply_rx)\n"
    "        }\n\n"
    "        pub(crate) fn query_virtualized_item_on_mta(\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "                WorkerCommand::VerifySetValue {\n"
    "                    attachment,\n"
    "                    request,\n"
    "                    reply,\n"
    "                } => {\n"
    "                    let _ = reply.send(state.verify_set_value(&attachment, request));\n"
    "                }\n",
    "                WorkerCommand::DispatchVerifiedInput {\n"
    "                    attachment,\n"
    "                    request,\n"
    "                    reply,\n"
    "                } => {\n"
    "                    let _ = reply.send(state.dispatch_verified_input(&attachment, request));\n"
    "                }\n"
    "                WorkerCommand::VerifySetValue {\n"
    "                    attachment,\n"
    "                    request,\n"
    "                    reply,\n"
    "                } => {\n"
    "                    let _ = reply.send(state.verify_set_value(&attachment, request));\n"
    "                }\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        fn dispatch_pattern(\n",
    "        fn dispatch_verified_input(\n"
    "            &self,\n"
    "            attachment: &WindowsUiaAttachment,\n"
    "            request: WindowsUiaVerifiedInputRequest,\n"
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n"
    "            if request.provider_incarnation_ref != self.provider_incarnation_ref\n"
    "                || request.provider_incarnation_ref != attachment.provider_incarnation_ref\n"
    "                || request.target_incarnation_ref != attachment.target_incarnation_ref\n"
    "                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref\n"
    "                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref\n"
    "                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref\n"
    "            {\n"
    "                return Err(WindowsUiaWorkerError::InvalidVerifiedInputRequest);\n"
    "            }\n\n"
    "            let context = self.revalidate_dispatch_context(\n"
    "                attachment,\n"
    "                WindowsUiaDispatchContextRequest {\n"
    "                    snapshot_cut_ref: request.snapshot_cut_ref.clone(),\n"
    "                    element_ref: request.element_ref.clone(),\n"
    "                    requirements: request.context_requirements,\n"
    "                },\n"
    "            )?;\n"
    "            let keyboard_state = snapshot_windows_keyboard_state()?;\n"
    "            evaluate_windows_keyboard_state(&keyboard_state).map_err(|error| match error {\n"
    "                WindowsInputDispatchBlocker::InputStateConflict => {\n"
    "                    WindowsVerifiedInputBoundaryError::InputStateConflict\n"
    "                }\n"
    "                other => WindowsVerifiedInputBoundaryError::InvalidInsertionResult(other),\n"
    "            })?;\n\n"
    "            let raw = windows_insert_verified_key_events(request.batch.events());\n"
    "            let expected = request.batch.len() as u32;\n"
    "            if raw.requested_event_count != expected {\n"
    "                return Err(WindowsVerifiedInputBoundaryError::RequestedCountMismatch {\n"
    "                    expected,\n"
    "                    reported: raw.requested_event_count,\n"
    "                }\n"
    "                .into());\n"
    "            }\n"
    "            let insertion_class = classify_windows_input_insertion(\n"
    "                expected,\n"
    "                raw.inserted_event_count,\n"
    "            )\n"
    "            .map_err(WindowsVerifiedInputBoundaryError::InvalidInsertionResult)?;\n\n"
    "            Ok(WindowsVerifiedInputBoundaryReceipt {\n"
    "                dispatch_context: context.observation,\n"
    "                keyboard_state,\n"
    "                requested_event_count: expected,\n"
    "                inserted_event_count: raw.inserted_event_count,\n"
    "                insertion_class,\n"
    "                raw_error_code: raw.raw_error_code,\n"
    "                reconciliation_required: true,\n"
    "            })\n"
    "        }\n\n"
    "        fn dispatch_pattern(\n",
)

# 3. Canonical public worker delegates to the exact inner MTA command.
replace_once(
    "crates/windows-uia-provider/src/subscription.rs",
    "    WindowsUiaSetValueVerificationReceipt, WindowsUiaSetValueVerificationRequest,\n",
    "    WindowsUiaSetValueVerificationReceipt, WindowsUiaSetValueVerificationRequest,\n"
    "    WindowsUiaVerifiedInputRequest, WindowsVerifiedInputBoundaryReceipt,\n",
)
replace_once(
    "crates/windows-uia-provider/src/subscription.rs",
    "        pub(crate) fn query_virtualized_item_on_mta(\n",
    "        pub fn dispatch_verified_input(\n"
    "            &self,\n"
    "            attachment: &WindowsUiaAttachment,\n"
    "            request: WindowsUiaVerifiedInputRequest,\n"
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n"
    "            self.inner.dispatch_verified_input(attachment, request)\n"
    "        }\n\n"
    "        pub(crate) fn query_virtualized_item_on_mta(\n",
)

# 4. Runtime preserves the existing consequential dispatch state machine.
runtime_module = Path("crates/windows-observe-runtime/src/verified_input_execution.rs")
if runtime_module.exists():
    raise SystemExit(f"unexpected pre-existing {runtime_module}")
runtime_module.write_text(
    "use localview_protocol::DispatchResult;\n"
    "use localview_windows_uia_provider::{\n"
    "    WindowsInputInsertionClass, WindowsVerifiedInputBoundaryReceipt,\n"
    "};\n\n"
    "/// Project a provider insertion receipt into the existing durable consequential\n"
    "/// dispatch states. This says only what the platform insertion boundary did;\n"
    "/// `DispatchedFull` is not world/postcondition success.\n"
    "pub fn dispatch_result_for_verified_input(\n"
    "    receipt: &WindowsVerifiedInputBoundaryReceipt,\n"
    ") -> DispatchResult {\n"
    "    match receipt.insertion_class {\n"
    "        WindowsInputInsertionClass::FullyInserted => DispatchResult::DispatchedFull,\n"
    "        WindowsInputInsertionClass::PartialDispatchUnknownOutcome => {\n"
    "            DispatchResult::DispatchedPartial\n"
    "        }\n"
    "        WindowsInputInsertionClass::ZeroInsertedBlocked => DispatchResult::NotDispatched,\n"
    "    }\n"
    "}\n",
    encoding="utf-8",
)
replace_once(
    "crates/windows-observe-runtime/src/lib.rs",
    "mod verified_action_coordinator;\nmod verified_execution;\n",
    "mod verified_action_coordinator;\nmod verified_execution;\nmod verified_input_execution;\n",
)
replace_once(
    "crates/windows-observe-runtime/src/lib.rs",
    "pub use verified_action_coordinator::*;\npub use verified_execution::*;\n",
    "pub use verified_action_coordinator::*;\npub use verified_execution::*;\npub use verified_input_execution::*;\n",
)

print("v4.3 verified-input GREEN patch applied")
