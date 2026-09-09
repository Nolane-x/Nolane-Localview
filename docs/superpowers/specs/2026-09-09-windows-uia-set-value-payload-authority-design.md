# Windows UIA SetValue Payload Authority Design

**Status:** approved architecture, implementation not yet started

**Base:** `main@ebb024cfaa6bdde0c647c2f875671d756c54dbfa`

**Scope:** the first payload-bearing consequential Windows semantic action: UI Automation `ValuePattern.SetValue` with exact payload authority, privacy-preserving durable commitment, fresh provider verification, and no blind retry.

## 1. Context

LocalView already has a verified Windows consequential action chain for payload-free semantic operations:

`Invoke -> SelectionItem -> Toggle -> Expand/Collapse`

The repository also already models `WindowsUiaPattern::Value` as a provider capability, but the trusted execution path does not yet dispatch `ValuePattern.SetValue` or publish current ValuePattern state for verified postconditions.

The existing correctness architecture deliberately separates:

- canonical operation identity;
- durable consequential intent;
- process-local confirmation authority;
- exact provider/target incarnation;
- exact fresh observation cut;
- provider dispatch evidence;
- independent post-dispatch world verification.

The existing durable operation binding is intentionally payload-free. Raw text, keys, coordinates, and similar transport data are not persisted in that record. SetValue must preserve that invariant rather than widening the old operation sidecar into a plaintext payload journal.

The product specification also requires semantic value mutation before keyboard fallback, distinguishes `set_value` from `insert_text`, and requires password/secure-field handling to remain separate and conservative.

## 2. Goal

Add one production-safe server-owned Windows route that can replace the value of one exact current non-sensitive UIA ValuePattern element while proving all of the following:

1. the exact text payload authorized at plan time is the payload used at dispatch time;
2. plaintext payload is never written to the consequential journal, operation sidecar, evidence store, semantic snapshot, logs, receipts, or HTTP response;
3. the exact canonical action binds a durable payload commitment before authorization can advance;
4. a daemon crash cannot reconstruct plaintext or redispatch authority from durable state;
5. provider dispatch success is not treated as world success;
6. a fresh post-dispatch provider cut independently proves the exact current element value commitment;
7. password, secure, read-only, unsupported, ambiguous, stale, or unreadable targets fail closed;
8. the existing Invoke/Select/Toggle/Expand/Collapse action chain is unchanged.

## 3. Non-goals

This slice does **not** add:

- keyboard text injection;
- `insert_text`, append, range replacement, or rich-text editing;
- pointer fallback;
- clipboard use;
- password or secure-field mutation;
- cross-platform AX/AT-SPI value execution;
- application-specific risk inference;
- automatic retry after unknown dispatch outcome;
- plaintext persistence for crash recovery;
- generic arbitrary payload support for every future action class.

An empty string is allowed and means exact replacement with an empty value. All other text-editing modes remain future work.

## 4. Chosen Architecture

Use a **volatile plaintext capability + durable payload commitment**.

The architecture has four independent bindings:

```text
canonical action operation
        SetValue
          |
          v
immutable durable payload commitment
  SHA-256 + UTF-8 byte length + mode
          |
          v
process-local one-shot plaintext capability
          |
          v
exact provider dispatch + fresh commitment verification
```

The operation record says **what class of side effect** is authorized. The payload commitment says **which exact payload** belongs to that admitted intent. The process-local capability carries the plaintext needed to execute the side effect. The postcondition contract proves the resulting world state without persisting plaintext.

These are intentionally separate trust objects.

## 5. Canonical Operation

Add:

```rust
CanonicalActionOperation::SetValue
```

`CanonicalActionOperation::InputText` remains unchanged for the legacy `BridgeActionKind::TypeText` compatibility path and for future keyboard/insert-text work.

The server-owned SetValue route binds `SetValue` explicitly with `record_intent_operation_bound_explicit`; it never derives SetValue from legacy `TypeText`.

This prevents a future keyboard insertion transport from being treated as the same side effect as semantic ValuePattern replacement.

## 6. Text Payload Domain

Initial SetValue accepts one JSON string field:

```json
{
  "element_ref": { "...": "..." },
  "value": "exact replacement text"
}
```

Rules:

- mode is fixed to `replace` and is server-owned;
- UTF-8 payload size is capped at **16 KiB**;
- empty string is valid;
- embedded NUL (`U+0000`) is rejected;
- no trimming, Unicode normalization, line-ending conversion, case folding, or other semantic rewriting occurs before commitment;
- the commitment is calculated over the exact UTF-8 bytes received after successful JSON decoding;
- if the target application normalizes or transforms text, fresh verification may fail. The first slice prefers a conservative false negative over silently changing payload semantics.

## 7. Durable Payload Commitment

Introduce an immutable companion record separate from the existing payload-free operation binding:

```rust
pub struct DurableCanonicalActionPayloadCommitment {
    pub action_id: Uuid,
    pub intent_journal_sequence: u64,
    pub operation: CanonicalActionOperation,
    pub payload_kind: CanonicalPayloadKind,
    pub mode: CanonicalTextMutationMode,
    pub sha256_hex: String,
    pub utf8_bytes: u32,
}

pub enum CanonicalPayloadKind {
    Utf8Text,
}

pub enum CanonicalTextMutationMode {
    Replace,
}
```

The record is valid only when:

- the consequential action is still exactly `Admitted`;
- the exact `IntentAdmitted` journal entry matches the canonical queued action;
- `operation == SetValue` for this slice;
- the companion file is created with `create_new` semantics;
- bytes are flushed and `sync_all()` completes before the method returns success;
- a second payload commitment for the same action is rejected rather than overwritten.

The companion path follows the existing operation-sidecar pattern and is bound to the intent journal sequence so a stale file cannot authorize another admission.

**Plaintext is never part of this durable record.**

Use SHA-256 for the payload commitment. Add `sha2 = "0.10"` as a workspace dependency and consume it only where commitment calculation is required.

## 8. Process-local Plaintext Capability

The control layer stores plaintext only in the process-local pending SetValue plan associated with the exact action ID and confirmation reference.

Conceptually:

```rust
struct PendingWindowsSetValuePayload {
    action_id: Uuid,
    commitment: DurableCanonicalActionPayloadCommitment,
    plaintext: String,
}
```

Requirements:

- it is never serialized;
- it is never cloned into durable journal structures;
- Debug output must not include plaintext;
- HTTP responses contain only action/confirmation/commitment metadata, never plaintext;
- logging/error messages contain lengths or opaque refs only;
- exact confirmation consumption removes the payload from the pending registry before verified execution begins;
- wrong confirmation does not expose or consume another action's payload;
- detach, session removal, runtime replacement, or daemon shutdown drops all pending payloads;
- daemon restart begins with no plaintext capabilities even if durable payload commitments exist.

This design guarantees that durable recovery can know **what commitment was intended** without recovering the secret needed to redispatch.

The implementation does not claim cryptographic memory zeroization in this slice; it guarantees process-local lifetime and no persistence/logging. Memory-hard zeroization can be a separate hardening slice if threat modeling requires it.

## 9. Server-owned Planning Route

Add:

```text
POST /v1/sessions/{id}/windows-observe/consequential/set-value/plan
```

Request fields are exactly:

```text
element_ref
value
```

Unknown fields are rejected.

The caller cannot provide or override:

- canonical operation;
- ValuePattern requirement;
- text mutation mode;
- payload digest;
- payload length;
- risk class;
- idempotency class;
- decision or acting principal;
- authorization revision;
- provider/target incarnation;
- expected SetValue postcondition contract.

Planning order:

```text
authenticate local control bearer
-> require live session/runtime/journal
-> validate payload bounds
-> serialize under existing plan gate
-> refresh exact UIA action evidence
-> require exact current element rebind
-> require ValuePattern supported
-> require field security/read-only state known
-> reject password/secure/read-only targets
-> compute payload commitment
-> create server-owned exact-value postcondition contract
-> bind direct canonical action with SetValue
-> record durable IntentAdmitted
-> fsync payload-free SetValue operation binding
-> fsync immutable payload commitment
-> create process-local confirmation + plaintext capability
-> return plan metadata
```

No durable authorization/PREPARED state is created before the operation binding and payload commitment are both durable.

## 10. Conservative Risk and Retry Policy

Generic UIA ValuePattern capability does not prove application-level consequences. Initial SetValue therefore keeps the same conservative floor as the existing generic semantic writes:

```text
risk_class = S4 destructive_or_irreversible
idempotency_class = irreversible
```

This is intentionally stricter than many ordinary text fields.

The action still requires explicit process-local confirmation.

No automatic retry is allowed after confirmation consumption or after a possibly dispatched outcome. A new attempt requires a new plan, new action ID, new payload commitment, new confirmation, and fresh evidence.

## 11. UIA Provider Observation

The Windows provider already reports whether ValuePattern is supported. Extend provider-owned semantic observation with ValuePattern state without publishing plaintext.

For a ValuePattern-capable node, observe:

```text
windows_uia.value.is_read_only = true|false
windows_uia.value.is_password = true|false
```

For a non-password node whose current value can be read, also publish:

```text
windows_uia.value.sha256 = <lowercase hex SHA-256 of exact UTF-8 value>
windows_uia.value.utf8_bytes = <decimal byte length>
```

Do **not** publish `CurrentValue` plaintext.

Password/secure behavior:

- if `IsPassword == true`, do not read/publish current value commitment and mark the node as sensitive;
- SetValue planning is blocked;
- the absence of a password value commitment is intentional redaction, not proof that the value is empty.

Read-only behavior:

- `ValuePattern.CurrentIsReadOnly == true` blocks planning;
- unknown/unreadable read-only state blocks planning.

Observation failures:

- if ValuePattern is supported on a non-password node but current value state required for correctness cannot be read, publish explicit provider debt and make reconciliation incomplete;
- incomplete observation cannot authorize SetValue or satisfy its postcondition.

## 12. Provider Dispatch Payload

Add:

```rust
WindowsUiaPatternDispatchOperation::SetValue
```

with:

```text
required_pattern() == WindowsUiaPattern::Value
```

Extend `WindowsUiaPatternDispatchRequest` with a typed process-local payload field rather than a raw untyped string:

```rust
pub enum WindowsUiaPatternDispatchPayload {
    SetValue {
        plaintext: String,
        commitment: CanonicalTextPayloadCommitment,
    },
}
```

Validation rules at the worker boundary:

- SetValue requires exactly one SetValue payload;
- all payload-free operations reject a payload;
- operation/pattern mismatch is rejected;
- worker recomputes SHA-256 + UTF-8 byte length from plaintext immediately before calling UIA;
- recomputed commitment must equal the exact durable commitment bound to the action;
- the live element must still support ValuePattern;
- the live ValuePattern must still be non-read-only;
- security/password state must still be non-sensitive at the final boundary;
- existing provider/target/snapshot/element/context fences still apply.

Only after those checks may the MTA worker call the exact live `IUIAutomationValuePattern::SetValue` method once.

The dispatch receipt contains the commitment metadata but never plaintext.

Provider return success remains only dispatch evidence.

## 13. Fresh Exact-value Postcondition Contract

SetValue must not depend on a caller-selected generic postcondition such as “an Edit node exists”. The server creates a correctness contract from the exact refreshed target identity and exact payload commitment.

Add a new immutable shared registry schema:

```text
native-semantic:v3
```

V1 and V2 remain byte-for-byte and semantically frozen.

V3 initial purpose is exact-node attribute equality, not arbitrary business logic.

Conceptual type:

```rust
pub struct NativeSemanticExactNodeAttributesPostconditionV3 {
    pub provider_family: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub opaque_provider_element_id: String,
    pub lifetime_profile_revision: String,
    pub required_attributes: BTreeMap<String, String>,
}
```

For SetValue the server-generated required attributes are exactly:

```text
windows_uia.value.sha256 = expected digest
windows_uia.value.utf8_bytes = expected byte length
windows_uia.value.is_password = false
windows_uia.value.is_read_only = false
```

Evaluation rules:

- snapshot must be complete;
- provider and target incarnations must match the contract;
- exactly one current semantic node may match provider family + opaque provider element identity + lifetime profile within that bound lineage;
- zero or ambiguous identity is `Unknown`, never pass;
- all required attributes equal -> `VerifiedPass`;
- exact unique node with readable complete attributes that differ -> `VerifiedFail`;
- missing correctness attributes or incomplete provider observation -> `Unknown`.

The post-dispatch snapshot has a new acquisition cut, so V3 intentionally does not require equality of the old `acquisition_cut_ref`; the action envelope and reconciliation receipt bind the observation cuts while V3 binds exact provider/target lineage and opaque element identity.

This schema is provider-neutral enough to support future AX/AT-SPI exact-attribute commitments without encoding Windows UIA business semantics into the shared registry.

## 14. Confirmation and Dispatch Order

SetValue confirmation reuses the existing two-phase route:

```text
POST /v1/sessions/{id}/windows-observe/consequential/{action_id}/confirm
```

Required order:

```text
validate bearer/session/action/confirmation identity
-> resolve exact current Windows runtime and journal
-> atomically remove exact pending plan + plaintext payload
-> recompute payload commitment and compare durable binding
-> create one-shot independent authorization revalidator
-> existing canonical preflight
-> exact live-element lease
-> canonical operation revalidation
-> payload commitment revalidation
-> volatile dispatch-context seal
-> durable PREPARED
-> second dispatch-time context arm
-> one-shot ValuePattern.SetValue
-> provider dispatch receipt without plaintext
-> fresh runtime-owned semantic snapshot
-> evaluate server-owned V3 exact-value postcondition
-> durable postcondition receipt
-> commit only on VerifiedPass
```

Any failure after confirmation consumption does not recreate plaintext authority or confirmation authority.

## 15. Crash and Recovery Semantics

Crash before confirmation:

- durable intent/operation/payload commitment may exist;
- plaintext and confirmation disappear;
- no dispatch can be reconstructed.

Crash after PREPARED:

- existing consequential journal semantics determine whether dispatch is known, unknown, or requires reconciliation;
- no blind redispatch is permitted;
- durable payload commitment and V3 postcondition contract allow fresh reality to be compared with the intended value commitment without recovering plaintext.

Crash after provider dispatch but before commit:

- fresh restart reconciliation may prove the exact value commitment and advance world outcome under the existing recovery model;
- dispatch authority itself is never restored.

## 16. Error Taxonomy

Add stable fail-closed SetValue errors at the appropriate layer, including:

```text
windows_set_value_payload_too_large
windows_set_value_payload_contains_nul
windows_set_value_sensitive_field_blocked
windows_set_value_read_only
windows_set_value_value_state_unknown
windows_set_value_payload_commitment_missing
windows_set_value_payload_commitment_mismatch
windows_set_value_payload_capability_missing
windows_set_value_pattern_unavailable
```

Provider-native HRESULT/error details may be preserved diagnostically but do not replace the stable LocalView semantic error.

No error string may contain plaintext payload.

## 17. Test Strategy

All production changes use explicit RED -> GREEN TDD.

### Layer 1 — shared payload commitment durability

Test before implementation:

- payload commitment API does not exist -> compile RED;
- exact intent + SetValue binds one immutable commitment;
- reopen preserves commitment but never plaintext;
- wrong action/intent sequence/corrupt sidecar fails closed;
- second binding cannot overwrite first;
- payload-free operation binding remains unchanged.

### Layer 2 — native-semantic V3 exact-node attribute contract

Test before implementation:

- V3 symbols/registry entry absent -> RED;
- V1/V2 golden references remain byte-for-byte unchanged;
- exact unique matching node passes;
- exact unique mismatched commitment fails;
- zero/duplicate identity is Unknown;
- incomplete observation is Unknown;
- unknown fields/non-canonical encoding fail closed.

### Layer 3 — Windows provider Value observation

Real and deterministic fixtures prove:

- ValuePattern capability is already detected;
- non-password editable value publishes digest + length, not plaintext;
- read-only state is explicit;
- password state is explicit and plaintext/commitment is not exported;
- unreadable non-sensitive Value state makes observation incomplete;
- existing SelectionItem/Toggle/ExpandCollapse state remains unchanged.

### Layer 4 — provider SetValue dispatch

Contract RED first:

- missing `SetValue` dispatch operation/payload types;
- operation/pattern mismatch rejected;
- missing payload rejected;
- payload on Invoke/Select/Toggle/Expand/Collapse rejected;
- commitment mismatch rejected before side effect;
- live read-only/password transition at final boundary rejects dispatch;
- exact `SetValue()` is called once;
- receipt contains commitment only.

### Layer 5 — HTTP planning authority

RED route test proves 404 before implementation, then requires:

- route exists;
- only `element_ref` + `value` accepted;
- client cannot forge operation/pattern/mode/digest/risk/idempotency/postcondition;
- over-limit/NUL/sensitive/read-only/unknown target rejected before durable authorization;
- success returns action ID + confirmation ref + non-secret commitment metadata;
- plaintext absent from response and journal bytes.

### Layer 6 — real Win32 SetValue end-to-end

Use a retained Win32 Edit control fixture and real UIA worker:

```text
initial fresh snapshot
-> SetValue plan
-> confirm
-> exact live ValuePattern.SetValue
-> fresh snapshot
-> V3 exact value commitment VerifiedPass
-> durable COMMITTED / world_outcome=verified_expected
```

Also test:

- wrong confirmation leaves exact confirmation usable;
- correct confirmation is one-shot;
- read-only Edit fails closed;
- password Edit fails closed without plaintext observation;
- same action cannot dispatch twice;
- post-dispatch unexpected transformed value does not commit success.

### Layer 7 — permanent Windows workflow gate

Add named Windows UIA workflow gates for:

- payload commitment authority contract;
- native-semantic V3 exact-node attributes;
- ValuePattern observation/privacy;
- SetValue dispatch worker;
- SetValue HTTP authority;
- real HTTP SetValue -> verified durable commit.

Final merge requires exact-head:

- full Rust workspace Check + Clippy + Tests on Ubuntu/macOS/Windows;
- Tauri + frontend;
- WebKitGTK/WKWebView/WebView2 rendered-pixel smokes;
- Windows UIA Observe including the new SetValue chain;
- no unresolved review blocker;
- no head drift before merge;
- post-merge CI + Windows UIA green on the exact merge SHA.

## 18. Files / Responsibility Boundaries

Expected focused changes:

- `crates/live-bridge/src/action_envelope.rs`
  - add server-owned `SetValue` canonical operation only.
- `crates/live-bridge/src/consequential_journal/payload_commitment.rs`
  - immutable durable payload commitment sidecar.
- `crates/live-bridge/src/consequential_journal.rs`
  - export focused payload commitment module.
- `crates/live-bridge/Cargo.toml` and workspace `Cargo.toml`
  - SHA-256 dependency.
- `crates/postcondition-contracts/src/lib.rs`
  - frozen V1/V2 plus new exact-node-attribute V3 registry schema.
- `crates/windows-uia-provider/src/pattern_dispatch.rs`
  - typed SetValue dispatch operation/payload/receipt commitment.
- `crates/windows-uia-provider/src/lib.rs`
  - ValuePattern state observation and exact SetValue COM call.
- `crates/windows-observe-runtime`
  - route-independent preflight/execution/postcondition plumbing for SetValue payload commitment.
- `crates/control/src/windows_consequential.rs`
  - SetValue plan route, process-local plaintext capability, exact confirmation handoff.
- focused tests under the corresponding crates plus `.github/workflows/windows-uia-observe.yml`.

Do not split or refactor unrelated provider/runtime/control code merely to make this feature prettier.

## 19. Safety Invariants

The implementation is unacceptable if any of these become false:

1. `SessionId` continuity is never execution authority.
2. capability evidence is never action authority.
3. operation identity and payload commitment are distinct durable records.
4. plaintext is never durable correctness state.
5. durable commitment cannot recreate plaintext or confirmation authority.
6. payload commitment must match immediately before provider side effect.
7. exact provider/target/element/context fences remain mandatory.
8. password/secure target mutation is unsupported in this slice.
9. provider dispatch acknowledgement is not world-state proof.
10. post-dispatch proof comes from a fresh provider observation.
11. unknown/incomplete/ambiguous evidence fails closed.
12. unknown outcome never enables blind retry.
13. V1/V2 postcondition wire semantics remain frozen.
14. existing payload-free Windows actions keep their current behavior.

## 20. Definition of Done

SetValue is complete only when one exact final head proves all of the following:

- server-owned semantic `SetValue` route exists;
- canonical `SetValue` is distinct from legacy/future `InputText`;
- exact payload commitment is durable before authorization advances;
- plaintext is process-local only and absent from durable/log/evidence outputs;
- ValuePattern observation publishes privacy-preserving current-value commitment;
- password and read-only targets fail closed;
- worker revalidates plaintext against the durable commitment before one-shot `SetValue`;
- server-owned V3 exact-node postcondition independently verifies the fresh current value commitment;
- crash/restart cannot reconstruct dispatch authority;
- real Win32 SetValue reaches durable `VERIFIED_EXPECTED` only after fresh proof;
- all existing Windows semantic action regressions stay green;
- exact-head CI and Windows UIA are green;
- exact merge SHA is post-merge green before the slice is declared closed.
