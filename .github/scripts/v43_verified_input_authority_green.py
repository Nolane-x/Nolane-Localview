from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one anchor, found {count}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")

# Journal permit exposes its already-durable preparation receipt identity, but
# remains opaque/non-Clone and still must be consumed by durable linearization.
replace_once(
    "crates/live-bridge/src/consequential_journal/base.rs",
    "    pub fn preparation_journal_sequence(&self) -> u64 {\n        self.preparation_journal_sequence\n    }\n}\n\n#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]",
    "    pub fn preparation_journal_sequence(&self) -> u64 {\n        self.preparation_journal_sequence\n    }\n\n    pub fn preparation_receipt_ref(&self) -> &str {\n        &self.preparation_receipt_ref\n    }\n}\n\n#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]",
)

# SHA-256 binds the exact ordered key batch into provider evidence.
replace_once(
    "crates/windows-uia-provider/Cargo.toml",
    "localview-protocol = { path = \"../protocol\" }\nthiserror.workspace = true\n",
    "localview-protocol = { path = \"../protocol\" }\nsha2.workspace = true\nthiserror.workspace = true\n",
)

p = Path("crates/windows-uia-provider/src/verified_input.rs")
text = p.read_text(encoding="utf-8")
text = text.replace(
    "use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};\nuse thiserror::Error;\n",
    "use localview_live_bridge::DispatchExecutionPermit;\nuse localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};\nuse sha2::{Digest, Sha256};\nuse thiserror::Error;\n",
    1,
)
old = '''#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputRequest {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub batch: WindowsVerifiedKeyboardBatch,
}
'''
new = '''#[derive(Debug, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputRequest {
    pub(crate) dispatch_attempt_ref: Uuid,
    pub(crate) action_id: Uuid,
    pub(crate) preparation_journal_sequence: u64,
    pub(crate) preparation_receipt_ref: String,
    pub(crate) snapshot_cut_ref: String,
    pub(crate) provider_incarnation_ref: ProviderIncarnationRef,
    pub(crate) target_incarnation_ref: TargetIncarnationRef,
    pub(crate) element_ref: ProviderElementRef,
    pub(crate) context_requirements: WindowsUiaDispatchContextRequirements,
    pub(crate) batch_digest: String,
    pub(crate) batch: WindowsVerifiedKeyboardBatch,
}

impl WindowsUiaVerifiedInputRequest {
    pub fn from_execution_permit(
        permit: &DispatchExecutionPermit,
        snapshot_cut_ref: String,
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        element_ref: ProviderElementRef,
        context_requirements: WindowsUiaDispatchContextRequirements,
        batch: WindowsVerifiedKeyboardBatch,
    ) -> Self {
        let batch_digest = verified_keyboard_batch_digest(&batch);
        Self {
            dispatch_attempt_ref: Uuid::new_v4(),
            action_id: permit.action_id(),
            preparation_journal_sequence: permit.preparation_journal_sequence(),
            preparation_receipt_ref: permit.preparation_receipt_ref().to_owned(),
            snapshot_cut_ref,
            provider_incarnation_ref,
            target_incarnation_ref,
            element_ref,
            context_requirements,
            batch_digest,
            batch,
        }
    }

    pub fn dispatch_attempt_ref(&self) -> Uuid { self.dispatch_attempt_ref }
    pub fn action_id(&self) -> Uuid { self.action_id }
    pub fn preparation_journal_sequence(&self) -> u64 { self.preparation_journal_sequence }
    pub fn preparation_receipt_ref(&self) -> &str { &self.preparation_receipt_ref }
    pub fn snapshot_cut_ref(&self) -> &str { &self.snapshot_cut_ref }
    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef { &self.provider_incarnation_ref }
    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef { &self.target_incarnation_ref }
    pub fn element_ref(&self) -> &ProviderElementRef { &self.element_ref }
    pub fn context_requirements(&self) -> WindowsUiaDispatchContextRequirements { self.context_requirements }
    pub fn batch_digest(&self) -> &str { &self.batch_digest }
    pub fn batch(&self) -> &WindowsVerifiedKeyboardBatch { &self.batch }
}

fn verified_keyboard_batch_digest(batch: &WindowsVerifiedKeyboardBatch) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"localview.windows.verified-input-batch.v1\\0");
    for event in batch.events() {
        hasher.update(event.virtual_key.to_le_bytes());
        hasher.update([match event.transition {
            WindowsKeyTransition::KeyDown => 0_u8,
            WindowsKeyTransition::KeyUp => 1_u8,
        }]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
'''
if text.count(old) != 1:
    raise SystemExit("verified_input.rs: request anchor drift")
text = text.replace(old, new, 1)
old = '''#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVerifiedInputBoundaryReceipt {
    pub dispatch_context: WindowsUiaDispatchContextObservation,
    pub keyboard_state: WindowsKeyboardStateSnapshot,
    pub requested_event_count: u32,
    pub inserted_event_count: u32,
    pub insertion_class: WindowsInputInsertionClass,
    pub raw_error_code: Option<u32>,
    pub reconciliation_required: bool,
}
'''
new = old + '''
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVerifiedInputReceipt {
    dispatch_attempt_ref: Uuid,
    action_id: Uuid,
    preparation_journal_sequence: u64,
    preparation_receipt_ref: String,
    snapshot_cut_ref: String,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    element_ref: ProviderElementRef,
    batch_digest: String,
    boundary: WindowsVerifiedInputBoundaryReceipt,
}

impl WindowsUiaVerifiedInputReceipt {
    pub(crate) fn from_request(
        request: WindowsUiaVerifiedInputRequest,
        boundary: WindowsVerifiedInputBoundaryReceipt,
    ) -> Self {
        Self {
            dispatch_attempt_ref: request.dispatch_attempt_ref,
            action_id: request.action_id,
            preparation_journal_sequence: request.preparation_journal_sequence,
            preparation_receipt_ref: request.preparation_receipt_ref,
            snapshot_cut_ref: request.snapshot_cut_ref,
            provider_incarnation_ref: request.provider_incarnation_ref,
            target_incarnation_ref: request.target_incarnation_ref,
            element_ref: request.element_ref,
            batch_digest: request.batch_digest,
            boundary,
        }
    }

    pub fn dispatch_attempt_ref(&self) -> Uuid { self.dispatch_attempt_ref }
    pub fn action_id(&self) -> Uuid { self.action_id }
    pub fn preparation_journal_sequence(&self) -> u64 { self.preparation_journal_sequence }
    pub fn preparation_receipt_ref(&self) -> &str { &self.preparation_receipt_ref }
    pub fn snapshot_cut_ref(&self) -> &str { &self.snapshot_cut_ref }
    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef { &self.provider_incarnation_ref }
    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef { &self.target_incarnation_ref }
    pub fn element_ref(&self) -> &ProviderElementRef { &self.element_ref }
    pub fn batch_digest(&self) -> &str { &self.batch_digest }
    pub fn boundary(&self) -> &WindowsVerifiedInputBoundaryReceipt { &self.boundary }
}
'''
if text.count(old) != 1:
    raise SystemExit("verified_input.rs: receipt anchor drift")
text = text.replace(old, new, 1)
p.write_text(text, encoding="utf-8")

# The exact MTA command now returns authority-bound evidence, while the raw
# boundary receipt remains an internal component of that evidence.
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        WindowsUiaVirtualizedItemRealizeRequest, WindowsVerifiedInputBoundaryError,\n        WindowsVerifiedInputBoundaryReceipt, classify_windows_input_insertion,\n",
    "        WindowsUiaVirtualizedItemRealizeRequest, WindowsUiaVerifiedInputReceipt,\n        WindowsVerifiedInputBoundaryError, WindowsVerifiedInputBoundaryReceipt,\n        classify_windows_input_insertion,\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "            reply: Sender<Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError>>,\n",
    "            reply: Sender<Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError>>,\n",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n            if request.dispatch_attempt_ref.is_nil()",
    "        ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {\n            if request.dispatch_attempt_ref.is_nil()",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n            if request.provider_incarnation_ref != self.provider_incarnation_ref",
    "        ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {\n            if request.provider_incarnation_ref != self.provider_incarnation_ref",
)
replace_once(
    "crates/windows-uia-provider/src/lib.rs",
    "            Ok(WindowsVerifiedInputBoundaryReceipt {\n                dispatch_context: context.observation,\n                keyboard_state,\n                requested_event_count: expected,\n                inserted_event_count: raw.inserted_event_count,\n                insertion_class,\n                raw_error_code: raw.raw_error_code,\n                reconciliation_required: true,\n            })\n        }\n\n        fn dispatch_pattern(",
    "            let boundary = WindowsVerifiedInputBoundaryReceipt {\n                dispatch_context: context.observation,\n                keyboard_state,\n                requested_event_count: expected,\n                inserted_event_count: raw.inserted_event_count,\n                insertion_class,\n                raw_error_code: raw.raw_error_code,\n                reconciliation_required: true,\n            };\n            Ok(WindowsUiaVerifiedInputReceipt::from_request(request, boundary))\n        }\n\n        fn dispatch_pattern(",
)

replace_once(
    "crates/windows-uia-provider/src/subscription.rs",
    "    WindowsUiaVerifiedInputRequest, WindowsVerifiedInputBoundaryReceipt,\n",
    "    WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,\n",
)
replace_once(
    "crates/windows-uia-provider/src/subscription.rs",
    "        ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> {\n            self.inner.dispatch_verified_input(attachment, request)\n",
    "        ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {\n            self.inner.dispatch_verified_input(attachment, request)\n",
)

replace_once(
    "crates/windows-uia-provider/tests/windows_verified_input_worker_mta_contract.rs",
    "    WindowsUiaAttachment, WindowsUiaVerifiedInputRequest, WindowsUiaWorker, WindowsUiaWorkerError,\n    WindowsVerifiedInputBoundaryReceipt,\n",
    "    WindowsUiaAttachment, WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,\n    WindowsUiaWorker, WindowsUiaWorkerError,\n",
)
replace_once(
    "crates/windows-uia-provider/tests/windows_verified_input_worker_mta_contract.rs",
    "    ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> =\n",
    "    ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> =\n",
)

print("v4.3 verified-input authority binding GREEN patch applied")
