# Windows UIA SetValue Payload Authority Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a production-safe Windows UIA `ValuePattern.SetValue` action whose plaintext remains process-local and one-shot, whose durable authority contains only an opaque keyed commitment, and whose success requires fresh exact post-dispatch equality verification.

**Architecture:** Preserve the existing payload-free canonical operation and provider-pattern paths. Add a dedicated SetValue payload authority sidecar in `localview-live-bridge`, an opaque payload-equality postcondition family, a SetValue-specific runtime/provider execution seam with zeroizing owned MTA buffers, and a server-owned two-phase control route. Existing Invoke/Select/Toggle/Expand/Collapse remain source-compatible and behaviorally unchanged.

**Tech Stack:** Rust 2024 / rust-version 1.85, Axum 0.8, Tokio, Serde/serde_json, UUID, windows-rs 0.61, HMAC-SHA256, OS CSPRNG, zeroize, existing consequential journal / Windows verified-action coordinator / GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-09-windows-set-value-payload-authority-design.md`

## Global Constraints

- No SetValue plaintext may become durable in journal files, operation/payload companions, semantic snapshots, evidence receipts, HTTP responses, logs, diagnostics, or workflow output.
- `CanonicalActionOperation` stays payload-free. `InputText` remains separate from `SetValue`.
- `WindowsUiaPatternDispatchRequest` stays payload-free and Clone; plaintext uses a dedicated non-Clone SetValue worker request.
- `replace_value` accepts at most 16 KiB UTF-8 and rejects U+0000; `clear_value` accepts no `value` field.
- No trimming, case folding, Unicode normalization, keyboard fallback, clipboard fallback, pointer fallback, or secret-field support.
- Password/unknown-password/read-only/unknown-read-only/unsupported ValuePattern targets fail closed before mutation.
- Generic SetValue risk stays `s4_destructive_or_irreversible`; idempotency stays `irreversible`.
- Exact confirmation consumes confirmation + payload authority once; provider uncertainty never creates retry authority.
- Provider acknowledgement is dispatch evidence only. Fresh equality `MATCH` on the exact action/payload/lineage/element is required before `VerifiedExpected` and durable commit.
- Restart cannot recreate payload plaintext, HMAC key, confirmation, or dispatch authority.
- Every production slice starts from a permanent failing test and preserves exact RED -> GREEN lineage.

---

### Task 1: Payload-Free Canonical SetValue Operation

**Files:**
- Modify: `crates/live-bridge/src/action_envelope.rs`
- Create: `crates/live-bridge/tests/canonical_set_value_operation_contract.rs`
- Modify only if needed for export: `crates/live-bridge/src/v43_lib.rs`

**Interfaces:**
- Produces `CanonicalActionOperation::SetValue`.
- Existing `CanonicalActionOperation::from_bridge_action_kind(BridgeActionKind::TypeText { .. })` continues returning `InputText`; it must never infer SetValue.

- [ ] **Step 1: Write the failing permanent operation contract**

The test must compile-reference the new variant and prove the legacy carrier still maps to `InputText`:

```rust
assert_eq!(format!("{:?}", CanonicalActionOperation::SetValue), "SetValue");
assert_eq!(
    CanonicalActionOperation::from_bridge_action_kind(&BridgeActionKind::TypeText {
        text: "caller-secret".into(),
    }),
    Some(CanonicalActionOperation::InputText),
);
```

Also direct-bind an empty `TypeText` carrier, call `take_public_actions`, and assert the direct canonical action never enters the public queue and contains no caller plaintext.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-live-bridge --test canonical_set_value_operation_contract -- --nocapture
```

Expected: compile FAIL because `CanonicalActionOperation::SetValue` does not exist.

- [ ] **Step 3: Add only the new payload-free enum variant**

```rust
pub enum CanonicalActionOperation {
    Activate,
    Select,
    Toggle,
    Expand,
    Collapse,
    SetValue,
    InputText,
    KeyInput,
    Scroll,
    Focus,
    Snapshot,
}
```

Do not change `from_bridge_action_kind` for TypeText.

- [ ] **Step 4: Run GREEN + operation-binding regression**

```bash
cargo test -p localview-live-bridge --test canonical_set_value_operation_contract -- --nocapture
cargo test -p localview-live-bridge --test v43_canonical_operation_binding -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add crates/live-bridge/src/action_envelope.rs crates/live-bridge/tests/canonical_set_value_operation_contract.rs
git commit -m "feat(live-bridge): add payload-free SetValue operation"
```

---

### Task 2: Durable Opaque SetValue Payload Binding

**Files:**
- Modify: root `Cargo.toml`
- Modify: `crates/live-bridge/Cargo.toml`
- Modify: `crates/live-bridge/src/consequential_journal.rs`
- Create: `crates/live-bridge/src/consequential_journal/set_value_payload.rs`
- Create: `crates/live-bridge/tests/v43_set_value_payload_binding.rs`

**Interfaces:**
- Produces `SetValueMode::{ReplaceValue, ClearValue}`.
- Produces `SetValuePayloadRef(Uuid)`.
- Produces `SetValueCommitmentKey::generate()` with 32 OS-random bytes and zeroize-on-drop storage.
- Produces `ConsequentialJournal::record_set_value_payload_binding(...)` and `set_value_payload_binding(action_id)`.
- Produces constant-time verifier `verify_set_value_payload_binding(...)`.

Use workspace dependencies compatible with Rust 1.85: `zeroize`, `hmac`, `sha2`, and `getrandom`. Keep exact versions in `Cargo.lock`; do not introduce `rand` unless compilation proves `getrandom` is insufficient.

Conceptual public metadata:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetValueMode { ReplaceValue, ClearValue }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SetValuePayloadRef(pub Uuid);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableSetValuePayloadBinding {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    pub payload_utf8_len: u64,
    pub commitment_algorithm: String,
    pub commitment_digest: Vec<u8>,
}
```

The key type must not derive `Clone`, `Serialize`, or plaintext-bearing `Debug`.

- [ ] **Step 1: Write RED tests for immutability, substitution fencing, and privacy**

Create one admitted action, operation-bind it as SetValue, generate a process key, and persist a payload binding for `b"secret-value"`. Assert:

```rust
assert_eq!(binding.payload_utf8_len, 12);
assert!(!serde_json::to_vec(&binding)?.windows(b"secret-value".len()).any(|w| w == b"secret-value"));
assert!(verify_set_value_payload_binding(&key, &binding, b"secret-value").is_ok());
assert!(verify_set_value_payload_binding(&key, &binding, b"different").is_err());
```

Reopen the journal and prove the durable metadata survives but a freshly generated process key cannot validate it. Attempt a second create for the same action and require create-new failure.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-live-bridge --test v43_set_value_payload_binding -- --nocapture
```

Expected: missing types/APIs.

- [ ] **Step 3: Implement canonical HMAC bytes exactly**

Canonical bytes:

```text
localview:set-value-payload:v1\0
+ action_id.as_bytes()
+ payload_ref UUID bytes
+ one-byte mode tag (0x01 replace, 0x02 clear)
+ payload length as big-endian u64
+ exact UTF-8 bytes
```

Use HMAC-SHA256 verification API rather than ordinary equality for MAC verification. Persist companion files using the existing operation-binding `create_new -> write_all -> flush -> sync_all` pattern and exact admitted intent journal sequence.

- [ ] **Step 4: Run GREEN + journal regressions**

```bash
cargo test -p localview-live-bridge --test v43_set_value_payload_binding -- --nocapture
cargo test -p localview-live-bridge --test v43_canonical_operation_binding -- --nocapture
cargo test -p localview-live-bridge --test v43_consequential_journal -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/live-bridge/Cargo.toml crates/live-bridge/src/consequential_journal.rs crates/live-bridge/src/consequential_journal/set_value_payload.rs crates/live-bridge/tests/v43_set_value_payload_binding.rs
git commit -m "feat(live-bridge): persist opaque SetValue payload commitments"
```

---

### Task 3: Opaque Payload-Equality Postcondition Schema

**Files:**
- Modify: `crates/postcondition-contracts/src/lib.rs`
- Create: `crates/postcondition-contracts/tests/payload_equality_registry.rs`

**Interfaces:**
- Adds schema `family="payload-equality", version="1"`.
- Produces `PayloadEqualityPostconditionContractV1 { mode, payload_ref }`.
- `PostconditionContractRegistry::decode` returns a typed registered payload-equality variant.
- `evaluate_native_semantic` returns `Unknown` for a valid payload-equality contract because generic semantic snapshots intentionally lack raw value authority.

Canonical ref:

```text
lvpc:payload-equality:v1:{"mode":"replace_value","payload_ref":"<uuid>"}
```

Keys must be canonical and exact; unknown fields fail closed.

- [ ] **Step 1: Write RED registry tests**

Prove round-trip canonicalization, unknown-field rejection, bad UUID rejection, and generic semantic evaluation -> `Unknown`.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-postcondition-contracts --test payload_equality_registry -- --nocapture
```

- [ ] **Step 3: Implement immutable schema/decoder**

Extend `STANDARD_SCHEMAS` and `RegisteredPostconditionContract`; add a dedicated error variant for payload-equality schema errors rather than reusing native-semantic errors.

- [ ] **Step 4: Run GREEN + existing registry suite**

```bash
cargo test -p localview-postcondition-contracts --test payload_equality_registry -- --nocapture
cargo test -p localview-postcondition-contracts --test native_semantic_registry -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add crates/postcondition-contracts/src/lib.rs crates/postcondition-contracts/tests/payload_equality_registry.rs
git commit -m "feat(postconditions): register opaque payload equality contracts"
```

---

### Task 4: Exact Dispatch-Operation Receipt Hardening

**Files:**
- Modify: `crates/windows-observe-runtime/src/execution_arm.rs`
- Modify: `crates/windows-observe-runtime/tests/execution_coordinator_behavior.rs`
- Modify focused mocks/tests that construct `WindowsUiaProviderExecutionReceipt`.

**Interfaces:**
- Adds `dispatch_operation: WindowsUiaPatternDispatchOperation` to `WindowsUiaProviderExecutionReceipt`.
- `provider_receipt_matches_request` compares `dispatch_operation` exactly.

- [ ] **Step 1: Write RED forged-receipt test**

For an admitted Expand action, have a fake executor echo every request field except `dispatch_operation = Collapse`; coordinator must return `ProviderReceiptMismatch` and must not record a successful dispatch linearization from the forged receipt.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior forged_dispatch_operation -- --nocapture
```

Expected: test demonstrates current receipt type cannot bind/distinguish the provider verb.

- [ ] **Step 3: Add exact operation to receipt and matcher**

Update all fake/provider receipt constructors to copy the request operation exactly.

- [ ] **Step 4: Run GREEN + Expand/Collapse regression**

```bash
cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior -- --nocapture
cargo test -p localview-windows-observe-runtime --test canonical_operation_authority_contract -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git commit -am "fix(windows-runtime): bind provider receipts to exact operation"
```

---

### Task 5: Windows Value Capability + Secure/Read-Only Facts

**Files:**
- Modify: `crates/windows-uia-provider/src/action_capability.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Modify: `crates/windows-uia-provider/src/event_buffer_lib.rs` if new types need export.
- Create: `crates/windows-uia-provider/tests/value_capability_contract.rs`
- Create/modify Windows-only smoke fixture test for Value capability.

**Interfaces:**
- `WindowsUiaPattern::Value` already exists; keep it.
- Adds non-sensitive exact capability facts to semantic node attributes:
  - `windows_uia.is_password = true|false|unknown`
  - `windows_uia.value.is_read_only = true|false|unknown`
- Adds typed provider-side evaluation used by plan-time and final-dispatch gates.

- [ ] **Step 1: Write RED capability contract**

Construct synthetic native semantic nodes and prove Value support alone is insufficient when password/read-only state is missing. Require explicit safe state:

```text
Value=Supported + is_password=false + value.is_read_only=false -> writable non-secure
anything unknown/true -> fail closed
```

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-uia-provider --test value_capability_contract -- --nocapture
```

- [ ] **Step 3: Capture safe capability facts without current value**

On Windows, query UIA password property and `IUIAutomationValuePattern::CurrentIsReadOnly` while building the action-capability snapshot. Do not read `CurrentValue` during planning capability capture.

- [ ] **Step 4: Run GREEN + real Windows smoke when CI executes**

Local cross-platform contract must pass; real fixture test remains `#[ignore]` + `LOCALVIEW_UIA_SMOKE=1` and runs in Windows workflow.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(windows-uia): expose safe ValuePattern capability facts"
```

---

### Task 6: Dedicated Non-Clone MTA SetValue Dispatch

**Files:**
- Modify: `crates/windows-uia-provider/Cargo.toml`
- Create: `crates/windows-uia-provider/src/set_value_dispatch.rs`
- Modify: `crates/windows-uia-provider/src/event_buffer_lib.rs`
- Modify: `crates/windows-uia-provider/src/lib.rs`
- Create: `crates/windows-uia-provider/tests/set_value_dispatch_contract.rs`
- Create: `crates/windows-uia-provider/tests/set_value_dispatch_worker_smoke.rs`

**Interfaces:**

```rust
pub struct WindowsUiaSetValueDispatchRequest {
    pub dispatch_attempt_ref: Uuid,
    pub action_id: Uuid,
    pub preparation_journal_sequence: u64,
    pub preparation_receipt_ref: String,
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub context_requirements: WindowsUiaDispatchContextRequirements,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    secret_utf8: Zeroizing<Vec<u8>>,
}
```

No `Clone`, serde, or plaintext-bearing Debug. Public constructor accepts exact metadata + bytes and validates the 16 KiB/NUL constraints again.

`WorkerCommand` gains `DispatchSetValue { attachment, request, reply }`; request is moved into the command exactly once.

- [ ] **Step 1: Write RED type/validation tests**

Prove oversized/NUL payload fails before worker side effect and that the public metadata/debug representation does not contain the plaintext sentinel.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-uia-provider --test set_value_dispatch_contract -- --nocapture
```

- [ ] **Step 3: Implement Windows final-boundary checks and exactly-one SetValue call**

Immediately before `SetValue`:

```text
exact current target
exact retained element/cut
ValuePattern available
IsPassword == false
CurrentIsReadOnly == false
final volatile context passes
```

Convert UTF-8 to the windows-rs string type adjacent to the call; do not log it. `clear_value` calls `SetValue("")` exactly once.

- [ ] **Step 4: Add real worker smoke**

Use an editable non-password Win32 fixture. Assert the value changes once and the receipt contains only payload ref/mode/lineage metadata.

- [ ] **Step 5: Run GREEN contracts**

```bash
cargo test -p localview-windows-uia-provider --test set_value_dispatch_contract -- --nocapture
cargo test -p localview-windows-uia-provider --test action_capability_contract -- --nocapture
```

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(windows-uia): add one-shot semantic SetValue dispatch"
```

---

### Task 7: Fresh SetValue Equality Verification

**Files:**
- Extend `crates/windows-uia-provider/src/set_value_dispatch.rs` or create `set_value_verification.rs` if dispatch file exceeds one focused responsibility.
- Modify exports in `event_buffer_lib.rs`.
- Modify: `crates/windows-observe-runtime/src/lib.rs`
- Create: `crates/windows-observe-runtime/src/set_value_execution.rs`
- Create: `crates/windows-observe-runtime/tests/set_value_execution_contract.rs`

**Interfaces:**

```rust
pub enum WindowsUiaSetValueEquality { Match, Mismatch, Unknown }

pub struct WindowsUiaSetValueVerificationReceipt {
    pub action_id: Uuid,
    pub payload_ref: SetValuePayloadRef,
    pub mode: SetValueMode,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
    pub observation_cut_ref: String,
    pub equality: WindowsUiaSetValueEquality,
}
```

The provider compares fresh `CurrentValue` inside the trusted boundary against the still-live expected bytes and returns equality only; raw current value never escapes.

Runtime produces `WindowsUiaSetValueExecutionPayload<'a>` and `execute_armed_uia_set_value_dispatch(...)`, reusing the existing one-shot armed permit lifecycle while requiring admitted operation `SetValue` + sealed pattern `Value`.

- [ ] **Step 1: RED runtime test**

Fake SetValue executor returns a valid dispatch receipt but equality `Mismatch`. Assert durable state cannot become `Committed`. Repeat with `Unknown`. `Match` may proceed to VerifiedExpected.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-observe-runtime --test set_value_execution_contract -- --nocapture
```

- [ ] **Step 3: Implement runtime SetValue-specific coordinator**

Do not put plaintext into `WindowsUiaProviderExecutionRequest`. Copy only into one temporary zeroizing `WindowsUiaSetValueDispatchRequest` at the MTA boundary; retain the authoritative process-local payload for post-dispatch verification.

- [ ] **Step 4: Implement fresh exact provider equality read**

After a possibly-dispatched action, acquire the existing consequential observation permit, revalidate exact lineage/element/password/read-only state, read current Value once, compare exact string semantics, return typed equality receipt.

- [ ] **Step 5: Run GREEN + payload-free runtime regressions**

```bash
cargo test -p localview-windows-observe-runtime --test set_value_execution_contract -- --nocapture
cargo test -p localview-windows-observe-runtime --test execution_coordinator_behavior -- --nocapture
cargo test -p localview-windows-observe-runtime --test postcondition_capture_contract -- --nocapture
```

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(windows-runtime): verify SetValue by fresh exact equality"
```

---

### Task 8: Server-Owned SetValue Plan + Process-Local Payload Lifecycle

**Files:**
- Modify: `crates/control/Cargo.toml`
- Modify: `crates/control/src/windows_consequential.rs`
- Modify if cleanup export is needed: `crates/control/src/lib.rs`
- Create: `crates/control/tests/windows_consequential_set_value_control.rs`
- Create: `crates/control/tests/windows_set_value_payload_privacy.rs`

**Interfaces:**
- Route: `POST /v1/sessions/{id}/windows-observe/consequential/set-value/plan`.
- Existing confirm route remains the only confirmation transition.
- `WindowsConsequentialControlHandle` gains one process-local 32-byte commitment key.
- Pending SetValue plan owns `ProcessLocalSetValuePayload` and confirmation ref.

Implement a request enum using `#[serde(tag="mode", rename_all="snake_case", deny_unknown_fields)]` so structural rules are impossible to blur:

```rust
#[derive(Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum WindowsSetValuePlanRequest {
    ReplaceValue { element_ref: ProviderElementRef, value: String },
    ClearValue { element_ref: ProviderElementRef },
}
```

- [ ] **Step 1: Write RED HTTP authority/privacy tests**

Require:
- forged `required_pattern`, `dispatch_verb`, `risk_class`, `commitment_digest` -> 422;
- clear mode with `value` -> 422;
- replace >16 KiB or U+0000 -> stable 422 error;
- runtime unavailable after structural validation -> 501/503 existing unavailable behavior without echoing plaintext;
- response never contains sentinel plaintext or HMAC digest.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-control --test windows_consequential_set_value_control -- --nocapture
```

- [ ] **Step 3: Implement exact planning order**

Under existing plan gate: auth -> session -> structure -> fresh evidence -> Value support -> safe password/read-only -> allocate refs -> direct-bind empty compatibility carrier -> IntentAdmitted -> explicit SetValue operation sidecar -> HMAC payload sidecar fsync -> publish pending confirmation/payload -> metadata response.

Use server-owned mandatory payload-equality postcondition contract; caller cannot supply arbitrary postconditions in this first route.

- [ ] **Step 4: Implement exact confirmation consumption**

Wrong confirmation leaves pending payload intact. Exact confirmation atomically removes pending plan, moves the non-Clone zeroizing payload into the execution transaction, loads exact durable binding, verifies HMAC/action/intent/payload-ref/mode/length, then continues existing authorization/PREPARED/arm flow.

Any failure after exact confirmation consumes and drops payload authority; no new confirmation is minted.

- [ ] **Step 5: Run GREEN + existing HTTP semantic-action regressions**

```bash
cargo test -p localview-control --test windows_consequential_set_value_control -- --nocapture
cargo test -p localview-control --test windows_set_value_payload_privacy -- --nocapture
cargo test -p localview-control --test windows_consequential_control -- --nocapture
cargo test -p localview-control --test windows_consequential_expand_collapse_control -- --nocapture
```

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(control): add one-shot SetValue payload authority"
```

---

### Task 9: Recovery / No-Redispatch Semantics

**Files:**
- Modify: `crates/live-bridge/src/postcondition_reconciliation.rs` only if registry plumbing needs a typed Unknown path.
- Modify: `crates/windows-observe-runtime/src/attached_recovery.rs`
- Create: `crates/windows-observe-runtime/tests/set_value_recovery_contract.rs`
- Create/modify focused control restart test if process-local pending state needs end-to-end proof.

**Interfaces:**
- Missing live SetValue payload/key during recovery yields typed payload-equality `Unknown`, not verifier infrastructure failure.
- `AUTHORIZED`, `PREPARED`, or `PossiblyDispatched` SetValue without live payload cannot redispatch.
- Existing `VerifiedUncommitted` commit-only recovery remains valid because expected postcondition evidence is already durable.

- [ ] **Step 1: Write RED crash-state tests**

Persist actions at each relevant durable state, reopen without the process-local payload/key, and assert no API can produce a provider dispatch request. For postcondition recovery, require `Unknown`/reconciliation-only rather than exception-based success or implicit retry.

- [ ] **Step 2: Run RED**

```bash
cargo test -p localview-windows-observe-runtime --test set_value_recovery_contract -- --nocapture
```

- [ ] **Step 3: Implement minimal typed recovery projection**

Do not recreate payload, HMAC key, or confirmation. Preserve existing commit-only recovery for VerifiedUncommitted.

- [ ] **Step 4: Run GREEN + generic recovery suites**

```bash
cargo test -p localview-windows-observe-runtime --test set_value_recovery_contract -- --nocapture
cargo test -p localview-windows-observe-runtime --test attachment_recovery_execution -- --nocapture
cargo test -p localview-windows-observe-runtime --test attachment_recovery_plan -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(windows-runtime): keep SetValue recovery reconciliation-only"
```

---

### Task 10: Real Windows End-to-End SetValue and Workflow Gates

**Files:**
- Create: `crates/control/tests/windows_consequential_set_value_windows_smoke.rs`
- Create/update: real provider SetValue fixture test.
- Modify: `.github/workflows/windows-uia-observe.yml`

**Interfaces:**
- Real smoke proves plan -> confirm -> exactly-one semantic SetValue -> fresh equality MATCH -> VerifiedExpected -> durable Committed.
- Separate tests prove password/read-only targets fail before mutation.

- [ ] **Step 1: Add ignored real Win32 HTTP smoke**

Use `LOCALVIEW_UIA_SMOKE=1` and a simple editable non-password fixture. Use a sentinel generated inside the test; never print it. Assert final fixture state directly in-process and assert durable/HTTP artifacts contain no sentinel bytes.

- [ ] **Step 2: Add named workflow gates**

Add these exact conceptual gates to `windows-uia-observe.yml`:

```text
V4.3 SetValue payload binding/privacy contract
V4.3 SetValue server-owned HTTP contract
Real Win32 ValuePattern writable/password/read-only preflight
Real retained Win32 UIA SetValue dispatch
Real HTTP SetValue to fresh equality verified durable commit
SetValue restart no-payload no-redispatch recovery
```

- [ ] **Step 3: Commit workflow + smoke tests**

```bash
git commit -am "test(windows): verify real SetValue authority and privacy"
```

---

### Task 11: Exact-Head Verification, Review, and Merge

**Files:**
- No production expansion. Only test/doc fixes supported by evidence.
- Update PR body with RED -> GREEN lineage and exact final SHA.

- [ ] **Step 1: Run focused local-equivalent suites**

```bash
cargo test -p localview-live-bridge
cargo test -p localview-postcondition-contracts
cargo test -p localview-windows-uia-provider
cargo test -p localview-windows-observe-runtime
cargo test -p localview-control
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- [ ] **Step 2: Privacy artifact scan**

Use a unique sentinel only inside tests and prove it is absent from all temp journal/companion files, serialized receipts, HTTP response bodies, and captured log buffers. Do not use broad repository grep as the sole privacy proof; inspect the exact artifacts generated by the test transaction.

- [ ] **Step 3: Whole-PR scope audit**

Compare against `main@ebb024cfaa6bdde0c647c2f875671d756c54dbfa`. Expected production scope is limited to live-bridge payload commitment, postcondition schema, Windows UIA provider/runtime, control SetValue lifecycle, dependency manifests, tests, workflow, and docs. Reject any Perception Budget, D2 surface ownership, Chromium governor, keyboard/pointer, macOS AX, or Linux AT-SPI authority changes.

- [ ] **Step 4: Exact PR head gate**

Open/update a draft PR from `feat/v43-windows-set-value-payload-authority` to `main`. Lock exact head SHA and require both:

```text
CI -> SUCCESS
Windows UIA Observe -> SUCCESS
```

Do not call the slice GREEN from partial job results.

- [ ] **Step 5: Review gate**

Check submitted reviews and review threads. Resolve Critical/Important issues through new RED -> GREEN cycles. Re-lock exact head after every fix.

- [ ] **Step 6: Merge with expected-head lock**

Merge only the exact verified head. Record the merge commit SHA.

- [ ] **Step 7: Post-merge verification**

Require `main` still equals the merge SHA and require post-merge push workflows `CI` and `Windows UIA Observe` both SUCCESS on that exact merge SHA before declaring the SetValue slice complete.
